#!/usr/bin/env bash
# 在本机完成完整验证和打包，候选提交的 CI 通过后才创建 tag 和 GitHub Release。
# 用法:
#   bash scripts/release.sh 1.6.0 "Concise English release notes." [--auto-engine|--rebuild-engine|--reuse-engine]
#   bash scripts/release.sh 1.6.0 --generate-notes --reuse-engine

set -euo pipefail

VERSION="${1:-}"
RELEASE_NOTES="${2:---generate-notes}"
ENGINE_MODE="${3:---auto-engine}"
TAG="${RELEASE_TAG:-${VERSION}}"
RELEASE_BRANCH="${RELEASE_BRANCH:-main}"
CI_WORKFLOW="${CI_WORKFLOW:-ci.yml}"
CI_DISCOVERY_TIMEOUT_SECONDS="${CI_DISCOVERY_TIMEOUT_SECONDS:-300}"

PKG_JSON="package.json"
TAURI_CONF="src-tauri/tauri.conf.json"
CARGO_TOML="src-tauri/Cargo.toml"
CARGO_LOCK="src-tauri/Cargo.lock"
PYPROJECT_TOML="pyproject.toml"
UV_LOCK="uv.lock"
ENGINE_ARCHIVE="src-tauri/resources/engine.tar.xz"
INSTALLER="src-tauri/target/release/bundle/nsis/SpeakFlow_${VERSION}_x64-setup.exe"
UPDATE_MANIFEST="$(dirname "$INSTALLER")/latest.json"
RELEASE_FILES=(
    "$PKG_JSON"
    "$TAURI_CONF"
    "$CARGO_TOML"
    "$CARGO_LOCK"
    "$PYPROJECT_TOML"
    "$UV_LOCK"
)

usage() {
    echo "用法: bash scripts/release.sh <X.Y.Z> [发布说明|--generate-notes] [--auto-engine|--rebuild-engine|--reuse-engine]"
}

fail() {
    echo "错误: $*" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || fail "缺少命令: $1"
}

preflight() {
    [[ -f "$TAURI_CONF" ]] || fail "请在项目根目录运行此脚本"
    [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
        usage
        fail "版本号必须是 X.Y.Z"
    }
    [[ "$ENGINE_MODE" == "--auto-engine" || "$ENGINE_MODE" == "--rebuild-engine" || "$ENGINE_MODE" == "--reuse-engine" ]] || {
        usage
        fail "未知引擎模式: $ENGINE_MODE"
    }

    for command_name in git gh node pnpm python uv cargo sha256sum; do
        require_command "$command_name"
    done
    find_seven_zip >/dev/null

    if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
        [[ -f "$HOME/.tauri/speakflow-updater.key" ]] || fail "Set TAURI_SIGNING_PRIVATE_KEY to the updater signing key"
        export TAURI_SIGNING_PRIVATE_KEY="$(cat "$HOME/.tauri/speakflow-updater.key")"
    fi
    export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"
    gh auth status >/dev/null

    if [[ -n "$(git status --porcelain)" ]]; then
        fail "工作区有未提交或未跟踪的改动，请先处理后再发版"
    fi

    local current_branch
    current_branch="$(git branch --show-current)"
    [[ "$current_branch" == "$RELEASE_BRANCH" ]] || {
        fail "只能从 ${RELEASE_BRANCH} 分支发版，当前分支为 ${current_branch:-detached HEAD}"
    }

    git fetch --quiet origin "$RELEASE_BRANCH"
    [[ "$(git rev-parse HEAD)" == "$(git rev-parse "origin/$RELEASE_BRANCH")" ]] || {
        fail "本地 ${RELEASE_BRANCH} 必须与 origin/${RELEASE_BRANCH} 完全一致"
    }

    ! git rev-parse --quiet --verify "refs/tags/$TAG" >/dev/null || {
        fail "本地 tag 已存在: $TAG"
    }
    ! git ls-remote --exit-code --tags origin "refs/tags/$TAG" >/dev/null 2>&1 || {
        fail "远端 tag 已存在: $TAG"
    }
}

update_versions() {
    echo "[1/9] 更新版本号 -> ${VERSION}"
    python - "$VERSION" <<'PY'
import json
import re
import sys
from pathlib import Path

version = sys.argv[1]

for path_str in ("package.json", "src-tauri/tauri.conf.json"):
    path = Path(path_str)
    data = json.loads(path.read_text(encoding="utf-8"))
    data["version"] = version
    path.write_text(
        json.dumps(data, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )

for path_str in ("src-tauri/Cargo.toml", "pyproject.toml"):
    path = Path(path_str)
    text = path.read_text(encoding="utf-8")
    updated, count = re.subn(
        r'^(version = ")[^"]+("\s*)$',
        rf'\g<1>{version}\2',
        text,
        count=1,
        flags=re.MULTILINE,
    )
    if count != 1:
        raise SystemExit(f"无法更新 {path_str} 的项目版本")
    path.write_text(updated, encoding="utf-8")

cargo_lock = Path("src-tauri/Cargo.lock")
text = cargo_lock.read_text(encoding="utf-8")
updated, count = re.subn(
    r'(\[\[package\]\]\s+name = "speak-flow"\s+version = ")[^"]+("\s*)',
    rf'\g<1>{version}\2',
    text,
    count=1,
)
if count != 1:
    raise SystemExit("无法更新 src-tauri/Cargo.lock 的根包版本")
cargo_lock.write_text(updated, encoding="utf-8")
PY

    uv lock
}

run_local_checks() {
    echo "[2/9] 运行快速本地检查"
    pnpm install --frozen-lockfile
    uv lock --check
    uv sync --frozen
    cargo fmt --manifest-path "$CARGO_TOML" --all -- --check
    git diff --check
}

ENGINE_REBUILT=false

prepare_engine() {
    echo "[4/9] 准备 Python 引擎"
    case "$ENGINE_MODE" in
        --reuse-engine)
            echo "按要求复用现有引擎归档。"
            ;;
        --auto-engine)
            if [[ -s "$ENGINE_ARCHIVE" ]] && python scripts/engine_fingerprint.py --check; then
                echo "引擎输入未变化，复用现有归档。"
            else
                echo "引擎输入已变化或缺少指纹，重新构建。"
                uv run --locked python scripts/build_engine.py
                ENGINE_REBUILT=true
            fi
            ;;
        *)
            uv run --locked python scripts/build_engine.py
            ENGINE_REBUILT=true
            ;;
    esac
}

find_seven_zip() {
    if command -v 7z >/dev/null 2>&1; then
        command -v 7z
    elif command -v 7z.exe >/dev/null 2>&1; then
        command -v 7z.exe
    elif [[ -x "/c/Program Files/7-Zip/7z.exe" ]]; then
        printf '%s\n' "/c/Program Files/7-Zip/7z.exe"
    elif [[ -x "/c/Program Files (x86)/7-Zip/7z.exe" ]]; then
        printf '%s\n' "/c/Program Files (x86)/7-Zip/7z.exe"
    else
        fail "未找到 7-Zip，无法验证引擎归档和 NSIS 安装包"
    fi
}

verify_engine_archive() {
    echo "[5/9] 验证引擎归档"
    node scripts/verify_engine_archive.mjs "$ENGINE_ARCHIVE"
    local seven_zip
    seven_zip="$(find_seven_zip)"
    "$seven_zip" t "$ENGINE_ARCHIVE"
    if [[ "$ENGINE_REBUILT" == true ]]; then
        python scripts/engine_fingerprint.py --write
    fi
}

build_installer() {
    echo "[6/9] 在本机构建安装包"
    pnpm tauri build
}

verify_installer() {
    echo "[7/9] 验证安装包"
    [[ -f "$INSTALLER" && -s "$INSTALLER" ]] || fail "安装包不存在或为空: $INSTALLER"
    local seven_zip
    seven_zip="$(find_seven_zip)"
    "$seven_zip" t "$INSTALLER"
    sha256sum "$INSTALLER"
    python scripts/generate_update_manifest.py "$VERSION" "$TAG" "$INSTALLER"
    [[ -z "$(git status --porcelain)" ]] || {
        git status --short >&2
        fail "构建产生了范围外改动或未跟踪文件"
    }
}

assert_expected_release_diff() {
    local changed_file allowed
    local -a unexpected=()
    local -a untracked=()
    while IFS= read -r -d '' changed_file; do
        allowed=false
        for release_file in "${RELEASE_FILES[@]}"; do
            if [[ "$changed_file" == "$release_file" ]]; then
                allowed=true
                break
            fi
        done
        if [[ "$allowed" == false ]]; then
            unexpected+=("$changed_file")
        fi
    done < <(git diff --name-only -z HEAD)

    while IFS= read -r -d '' changed_file; do
        untracked+=("$changed_file")
    done < <(git ls-files --others --exclude-standard -z)

    if (( ${#unexpected[@]} > 0 )); then
        printf '错误: 检查或构建产生了范围外改动:\n' >&2
        printf '  %s\n' "${unexpected[@]}" >&2
        exit 1
    fi
    if (( ${#untracked[@]} > 0 )); then
        printf '错误: 检查或构建产生了未跟踪文件:\n' >&2
        printf '  %s\n' "${untracked[@]}" >&2
        exit 1
    fi
}

commit_candidate() {
    echo "[3/9] 创建并推送候选提交"
    assert_expected_release_diff
    git add -- "${RELEASE_FILES[@]}"
    git diff --cached --check
    if git diff --cached --quiet; then
        echo "版本元数据未变化，复用当前 HEAD 作为候选提交。"
    else
        git commit -m "chore(release): bump version to ${VERSION}"
    fi
    git push origin "$RELEASE_BRANCH"
}

wait_for_ci() {
    local candidate_sha="$1"
    local deadline=$((SECONDS + CI_DISCOVERY_TIMEOUT_SECONDS))
    local run_id=""

    echo "等待候选提交 ${candidate_sha} 的 CI"
    while (( SECONDS < deadline )); do
        run_id="$(
            gh run list \
                --workflow "$CI_WORKFLOW" \
                --event push \
                --commit "$candidate_sha" \
                --limit 10 \
                --json databaseId \
                --jq '.[0].databaseId // empty'
        )"
        [[ -n "$run_id" ]] && break
        sleep 5
    done

    [[ -n "$run_id" ]] || fail "在 ${CI_DISCOVERY_TIMEOUT_SECONDS} 秒内未找到候选提交的 CI run"
    gh run watch "$run_id" --exit-status

    local run_head run_conclusion job_conclusion
    run_head="$(gh run view "$run_id" --json headSha --jq '.headSha')"
    run_conclusion="$(gh run view "$run_id" --json conclusion --jq '.conclusion')"
    [[ "$run_head" == "$candidate_sha" ]] || fail "CI run 的 headSha 与候选提交不一致"
    [[ "$run_conclusion" == "success" ]] || fail "CI 未成功: $run_conclusion"

    for required_job in Frontend Python Rust "Application formal contracts"; do
        job_conclusion="$(
            gh run view "$run_id" \
                --json jobs \
                --jq ".jobs[] | select(.name == \"$required_job\") | .conclusion"
        )"
        [[ "$job_conclusion" == "success" ]] || {
            fail "CI job ${required_job} 缺失或未成功: ${job_conclusion:-missing}"
        }
    done
}

publish_release() {
    local candidate_sha="$1"
    echo "[9/9] 创建 tag 并发布 GitHub Release"

    [[ "$(git rev-parse HEAD)" == "$candidate_sha" ]] || fail "HEAD 已离开候选提交"
    [[ -z "$(git status --porcelain)" ]] || fail "打 tag 前工作区不干净"
    git fetch --quiet origin "$RELEASE_BRANCH"
    [[ "$(git rev-parse "origin/$RELEASE_BRANCH")" == "$candidate_sha" ]] || {
        fail "origin/${RELEASE_BRANCH} 已离开候选提交，请重新建立发布候选"
    }

    git tag -a "$TAG" "$candidate_sha" -m "Release $TAG"
    git push origin "refs/tags/$TAG"

    if [[ "$RELEASE_NOTES" == "--generate-notes" ]]; then
        gh release create "$TAG" "$INSTALLER" "$INSTALLER.sig" "$UPDATE_MANIFEST" "$ENGINE_ARCHIVE" \
            --title "$TAG" \
            --generate-notes \
            --verify-tag \
            --fail-on-no-commits
    else
        gh release create "$TAG" "$INSTALLER" "$INSTALLER.sig" "$UPDATE_MANIFEST" "$ENGINE_ARCHIVE" \
            --title "$TAG" \
            --notes "$RELEASE_NOTES" \
            --verify-tag \
            --fail-on-no-commits
    fi
}

main() {
    preflight
    echo "=== 准备发布 ${TAG} ==="
    update_versions
    run_local_checks
    commit_candidate

    local candidate_sha ci_log ci_pid
    candidate_sha="$(git rev-parse HEAD)"
    ci_log="$(mktemp)"
    wait_for_ci "$candidate_sha" >"$ci_log" 2>&1 &
    ci_pid=$!
    trap 'kill "$ci_pid" 2>/dev/null || true' EXIT

    prepare_engine
    verify_engine_archive
    build_installer
    verify_installer

    echo "[8/9] 等待候选提交 ${candidate_sha} 的 CI"
    if ! wait "$ci_pid"; then
        cat "$ci_log" >&2
        fail "候选提交 ${candidate_sha} 的 CI 未通过"
    fi
    trap - EXIT
    tail -n 20 "$ci_log"
    publish_release "$candidate_sha"
    echo "=== 发布完成: https://github.com/khentmoba/SpeakFlow/releases/tag/${TAG} ==="
}

main "$@"
