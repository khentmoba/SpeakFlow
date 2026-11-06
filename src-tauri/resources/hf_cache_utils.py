"""HuggingFace 缓存检测共享工具模块

供各本地 ASR 服务与 download_models.py 共同使用，
避免重复实现缓存路径和模型就绪检查逻辑。
"""

import os
import json
import hashlib


def _load_asr_catalog():
    """Read the generated model catalog (scripts/sync_asr_catalog.py)."""
    path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "asr_catalog.json")
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)


ASR_CATALOG = _load_asr_catalog()

# Every catalog model runs through transcribe.cpp in the same resident server.
TRANSCRIBE_CPP_ASR_MODELS = {model["key"]: model for model in ASR_CATALOG["models"]}

R2T2_MODEL = {
    "repo_id": "davidxifeng/Confucius4-R2T2-gguf",
    "filename": "r2t2-q8_0.gguf",
    "revision": "a8e6b385d7df7eae9519363e07034a209004797a",
    "size": 2_477_512_064,
    "sha256": "19f5ccd624484bcb5d44301437de41560b0ecc40c430e8850dfeefefbe82ccf5",
}

LOCAL_ASR_MODELS = {**TRANSCRIBE_CPP_ASR_MODELS, "confucius4-r2t2": R2T2_MODEL}

_WEIGHT_EXTS = (".pt", ".bin", ".safetensors", ".onnx", ".gguf")
_MIN_WEIGHT_SIZE = 1_000_000  # 与 Rust 端阈值一致
COMPLETE_MANIFEST_NAME = ".speak_flow_complete.json"


def get_hf_cache_root():
    """返回 HuggingFace 缓存根目录

    优先级：HF_HUB_CACHE（由 Rust 设置的自定义路径）> HF_HOME/hub > 默认
    """
    hf_hub_cache = os.environ.get("HF_HUB_CACHE")
    if hf_hub_cache:
        return hf_hub_cache
    hf_home = os.environ.get("HF_HOME")
    if hf_home:
        return os.path.join(hf_home, "hub")
    return os.path.join(os.path.expanduser("~"), ".cache", "huggingface", "hub")


def is_hf_repo_ready(repo_id):
    """检查 HuggingFace 模型是否已缓存且包含实际模型权重文件。

    仅检查目录结构不够——下载中途取消会留下空壳目录（refs/snapshots 存在但无权重文件），
    导致后续加载卡死。这里额外验证 snapshots 中存在 >1MB 的模型权重文件。
    """
    return any(_snapshot_is_ready(path) for path in _snapshot_paths(repo_id))


def find_hf_snapshot_file(repo_id, filename):
    """Resolve one exact cached file without scanning unrelated quantizations.

    Falls back to the user's read-only model folders, which are searched only
    after the cache. Nothing is ever written to those folders.
    """
    for snapshot_path in _snapshot_paths(repo_id):
        if _snapshot_is_ready(snapshot_path, filename):
            return os.path.join(snapshot_path, filename.replace("/", os.sep))
    return find_shared_model_file(filename)


def model_search_dirs():
    """Read-only folders from `SPEAK_FLOW_MODEL_DIRS`, in search order."""
    raw = os.environ.get("SPEAK_FLOW_MODEL_DIRS", "")
    return [entry.strip() for entry in raw.split(os.pathsep) if entry.strip()]


def find_shared_model_file(filename):
    """First copy of `filename` in a shared folder whose size matches the catalog.

    The size rule is the same one Rust uses for readiness, so both sides pick the
    same file. A name that is not in the catalog is never loaded from a folder.
    """
    expected = next(
        (model["size"] for model in ASR_CATALOG["models"] if model["filename"] == filename),
        None,
    )
    if expected is None:
        return None
    for directory in model_search_dirs():
        candidate = os.path.join(directory, filename)
        try:
            if os.path.isfile(candidate) and os.path.getsize(candidate) == expected:
                return candidate
        except OSError:
            continue
    return None


def _snapshot_paths(repo_id):
    cache_root = get_hf_cache_root()
    repo_dir = os.path.join(cache_root, "models--" + repo_id.replace("/", "--"))
    snapshots_dir = os.path.join(repo_dir, "snapshots")
    try:
        names = sorted(os.listdir(snapshots_dir))
    except OSError:
        return []

    snapshot_names = []
    try:
        with open(os.path.join(repo_dir, "refs", "main"), "r", encoding="utf-8") as f:
            revision = f.read().strip()
        if revision in names:
            snapshot_names.append(revision)
    except (OSError, UnicodeError):
        pass
    snapshot_names.extend(name for name in names if name not in snapshot_names)
    return [
        os.path.join(snapshots_dir, name)
        for name in snapshot_names
        if os.path.isdir(os.path.join(snapshots_dir, name))
    ]


def _snapshot_is_ready(snapshot_path, required_file=None):
    if required_file is not None:
        candidate = os.path.join(snapshot_path, required_file.replace("/", os.sep))
        try:
            if not os.path.isfile(candidate) or os.path.getsize(candidate) < _MIN_WEIGHT_SIZE:
                return False
        except OSError:
            return False
    if os.path.lexists(os.path.join(snapshot_path, COMPLETE_MANIFEST_NAME)):
        return _snapshot_matches_completion_manifest(snapshot_path, required_file)
    return _snapshot_matches_legacy_weight_check(snapshot_path)


def cleanup_incomplete_files(repo_id):
    """删除旧 huggingface_hub blob 临时文件，保留 snapshots 下可续传的 partial。"""
    cache_root = get_hf_cache_root()
    dir_name = "models--" + repo_id.replace("/", "--")
    repo_dir = os.path.join(cache_root, dir_name)
    blobs_dir = os.path.join(repo_dir, "blobs")
    if not os.path.isdir(blobs_dir):
        return 0

    removed = 0
    for root, _dirs, files in os.walk(blobs_dir):
        for filename in files:
            if not filename.endswith(".incomplete"):
                continue
            path = os.path.join(root, filename)
            try:
                os.remove(path)
                removed += 1
            except OSError:
                pass
    return removed


def _snapshot_matches_completion_manifest(snapshot_path, required_file=None):
    manifest_path = os.path.join(snapshot_path, COMPLETE_MANIFEST_NAME)
    try:
        with open(manifest_path, "r", encoding="utf-8") as f:
            manifest = json.load(f)
    except (OSError, ValueError, UnicodeError):
        return False

    if not isinstance(manifest, dict):
        return False
    files = manifest.get("files")
    if not isinstance(files, list) or not files:
        return False

    has_weight = False
    has_required_file = required_file is None
    for item in files:
        if not isinstance(item, dict):
            return False
        rel_path = item.get("path")
        expected_size = item.get("size")
        expected_sha256 = item.get("sha256")
        if (
            not isinstance(rel_path, str)
            or type(expected_size) is not int
            or expected_size < 0
        ):
            return False
        if expected_sha256 is not None and (
            not isinstance(expected_sha256, str)
            or len(expected_sha256) != 64
            or any(char not in "0123456789abcdefABCDEF" for char in expected_sha256)
        ):
            return False
        if (
            not rel_path
            or os.path.isabs(rel_path)
            or os.path.splitdrive(rel_path)[0]
            or ".." in rel_path.replace("\\", "/").split("/")
        ):
            return False
        path = os.path.join(snapshot_path, rel_path.replace("/", os.sep))
        try:
            if not os.path.isfile(path):
                return False
            actual_size = os.path.getsize(path)
            if actual_size != expected_size:
                return False
            if expected_sha256 is not None and _sha256_file(path).lower() != expected_sha256.lower():
                return False
        except OSError:
            return False
        if rel_path == required_file:
            has_required_file = True
        if rel_path.endswith(_WEIGHT_EXTS) and actual_size >= _MIN_WEIGHT_SIZE:
            has_weight = True

    return has_weight and has_required_file


def _sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _snapshot_matches_legacy_weight_check(snapshot_path):
    has_weight = False
    for root, _dirs, files in os.walk(snapshot_path):
        for f in files:
            if f.endswith(".incomplete"):
                return False
            if f.endswith(_WEIGHT_EXTS):
                filepath = os.path.join(root, f)
                try:
                    if os.path.getsize(filepath) >= _MIN_WEIGHT_SIZE:
                        has_weight = True
                except OSError:
                    return False
    return has_weight
