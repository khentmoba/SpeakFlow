# Releases

Use the project skill `/skill:speakflow-release` to publish a signed in-app update. Run
`/reload` after adding the skill if it is not yet available in your session.

Build installers locally. GitHub Actions checks source code; it does not build or upload installers.
Write the README, documentation and release notes in concise English.

## Prepare

- Commit reviewed changes to `main` and synchronize it with `origin/main`.
- Keep the working tree clean. Confirm the target version and tag do not exist.
- Install the [development tools](development.md), GitHub CLI, Git Bash and 7-Zip.
- Authenticate with `gh auth status`.
- Preserve the updater signing key. It lives outside the repository at `~/.tauri/speakflow-updater.key` on the development PC, with access restricted to its owner and SYSTEM. Back it up securely; losing it prevents installed clients from trusting future updates. The public key is committed in `src-tauri/tauri.conf.json`.
- `release.sh` loads that key by default. Other builders must set `TAURI_SIGNING_PRIVATE_KEY` (key content or path) and, when applicable, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Never commit the private key.
- If the R2T2 source, patch or native dependencies changed, rebuild both native packages first; see [R2T2](r2t2-native.md).

## Publish

From the repository root:

```bash
bash scripts/release.sh 1.6.0 "Concise English release notes."
```

The script updates all six version files, runs quick local checks (lockfiles, Rust formatting, whitespace), and pushes the version bump as the candidate commit. While CI checks that exact commit, it builds the engine and installer, validates both archives, and prints the installer SHA-256. It then requires the candidate's `Frontend`, `Python`, `Rust` and `Application formal contracts` CI jobs to exist and succeed; skipped jobs block publication. Only after the build and CI both succeed and `origin/main` still matches does it publish the annotated tag and GitHub Release.

The full checks run once, in CI. If the build or CI fails, the version bump stays on `main` without a tag. Fix the cause and rerun the same command: an unchanged version reuses the existing candidate commit.

The default engine mode, `--auto-engine`, reuses `engine.tar.xz` only when `scripts/engine_fingerprint.py --check` matches the fingerprint stored beside it after its last build. The fingerprint covers the packaged Python sources, `pyproject.toml` and `uv.lock` (ignoring the app version), the engine build scripts and patches, and the R2T2 native manifests. Otherwise it rebuilds. `--rebuild-engine` always rebuilds. Use `--reuse-engine` only when the packaged Python code, native libraries and dependencies are unchanged and the existing archive has been verified. Release builds reject missing or invalid engine archives. Development/debug builds can use a placeholder.

For a manual release, follow the same gates: [local checks](development.md#checks), verified engine and installer, candidate commit, matching remote CI, then tag and Release. For multiline notes, use `gh release create --notes-file <file> --verify-tag`. Do not publish a tag for a failed candidate.

## Announce

Announce the published build in the project's Discord release channel. The
`/skill:speakflow-release` step 4 holds the destination, the poster and the
announcement format; run it rather than posting by hand, so a release produces
exactly one announcement.

## Recovery and edits

- Before tagging: fix the failure and validate a new candidate.
- If the tag exists but Release creation failed: verify its commit before retrying `gh release create --verify-tag`. Do not move a published tag.
- Title and release-note corrections can use `gh release edit <tag> --notes-file <file>` without changing tags or assets. Preserve the facts of the original release.
- Asset replacement is a separate release operation requiring explicit authorization; immutable releases prohibit it.

Building or publishing an installer does not update the application currently installed on the development machine.

## In-app updates

Use the pinned Tauri CLI 2.12.3 or newer: older signers can generate unusable keys with empty passwords. Update signatures must include the app version, which the bundler adds automatically.

Release builds create an installer and its `.sig` file. The release script generates `latest.json` with the version, Windows x64 installer URL, and signature, then uploads all three assets together. The app reads the latest release's feed, verifies the downloaded installer using its embedded public key, runs the Windows installer in passive mode, and reopens automatically. Do not publish `latest.json` without its matching installer and signature.

Versions through 1.0.1 only open a download page. Users need to install the first native-updater build once; subsequent releases can update inside Settings. Recording or transcription must finish before installation. Network or signature failures leave the installed app untouched and allow a retry.
