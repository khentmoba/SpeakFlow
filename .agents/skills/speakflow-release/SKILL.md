---
name: speakflow-release
description: Publish a SpeakFlow release with signed in-app updates, then announce it on Discord. Use when asked to trigger a release, ship a version, publish an app update, or announce a release.
---

# SpeakFlow release

Create an actual GitHub Release for `khentmoba/SpeakFlow`, not a release PR, and
announce the published build in the project's Discord release channel.
Creating or editing this skill is not authorization to publish.
Run commands from the repository root.

## 1. Establish the release candidate

Read `../../../docs/releasing.md`, `../../../scripts/release.sh` and
`../../../src-tauri/tauri.conf.json` relative to this skill directory. These are
the source of truth for release gates, signing and the updater endpoint.

- Inspect the working tree, branch, origin URL, local versions, remote tags and
  latest published release. Confirm origin is SpeakFlow's repository.
- Confirm an unused `X.Y.Z` version newer than both the source version and the
  latest published stable release. If the user supplied no version, propose a
  patch bump and ask for confirmation. Write concise English release notes from
  the changes since the previous release.
- Use a clean `main` checkout matching `origin/main`. If this thread is in a
  feature worktree, locate the existing main checkout and verify its state.
  Unmerged changes are a blocker: report what must reach main first rather than
  silently merging, resetting, stashing or dropping work.
- Check the script's required tools, GitHub authentication and signing-key
  availability without printing secret values. Preserve the existing updater key;
  generating a replacement would break trust for installed clients. If the key
  is unavailable, stop and ask the maintainer to provision it securely outside
  the repository.
- Use the default engine mode, `--auto-engine`: it rebuilds unless the stored
  input fingerprint matches (`python scripts/engine_fingerprint.py --check`).
  Pass `--rebuild-engine` when in doubt. If R2T2 native inputs changed, follow
  `../../../docs/r2t2-native.md` first.

Done when the version, notes, candidate checkout and engine mode are established,
and the release can run without bypassing a preflight check.

## 2. Build and publish

Run the existing script with the confirmed version and notes:

```bash
bash scripts/release.sh <X.Y.Z> "<English release notes>"
```

Add an engine flag only when step 1 established one. The script updates version
metadata, pushes the candidate commit, builds and validates the artifacts locally
while CI checks that exact commit, requires every CI job to pass, then publishes
the tag and assets. The candidate commit reaches `main` before the build, so a
failed run leaves an untagged version bump; rerunning the same version reuses it.
A tag push or workflow dispatch alone does not build or publish this app.

Stop on any failure and report the last successful stage, candidate SHA, CI URL
if available, and whether a commit, tag or release reached GitHub. Follow the
recovery section of `docs/releasing.md` before retrying; preserve published tags
and assets. Keep the CI and signing gates intact.

Done when the script succeeds and the published tag resolves to the checked
candidate commit.

## 3. Verify installed clients can discover the update

Inspect `gh release view <tag> --repo khentmoba/SpeakFlow --json url,tagName,isDraft,isPrerelease,assets`
and fetch the public endpoint configured in `plugins.updater.endpoints` with
cache bypass/retries as needed. Download the published assets to a temporary
directory rather than trusting only local build output.

Verify all of the following:

- The release is published, stable, and is the repository's latest release.
- Its assets include the versioned Windows x64 installer, matching `.sig`,
  `latest.json`, and `engine.tar.xz`.
- The public updater endpoint returns the confirmed version. Its
  `platforms.windows-x86_64.url` points to this release's installer and is
  downloadable; its signature equals the nonempty published `.sig` contents.
- The downloaded installer SHA-256 matches the locally validated installer.
- The configured updater public key and endpoint remain unchanged. Tauri updater
  signatures are distinct from Windows Authenticode; describe each accurately.

If the public feed is stale or an asset is missing, report publication as
incomplete for in-app updates. Do not replace assets without explicit approval.

Report the release URL, version/tag, candidate SHA, CI result and feed verification.
Distinguish these checks from a real installation test. Only claim end-to-end
update success after a maintainer-authorized test from an older compatible
installed build through Settings, including restart and version confirmation.
Publishing alone does not update the developer's installed app; versions through
1.0.1 need a one-time manual installation to gain the native updater.

## 4. Announce the release on Discord

The release is not finished until the announcement exists. Users find new builds
through the channel, not through the GitHub tag, so publishing without an
announcement ships a release nobody hears about.

- **Destination:** the **Felary** server (`1537573945992024125`), channel
  `#speakflow-releases` (`1562407200259837978`), posted by the
  **SpeakFlow Release Bot** webhook (`1562407289510436894`).
- **Poster:** `scripts/post_release_discord.mjs`. It reads the webhook ID and
  token from `~/.config/speakflow/discord-release-webhook.json`, so the token
  never reaches a command line or a transcript. It prints the payload and
  sends nothing until `--send` is passed.
- The Discord MCP tools post plain text only, and the announcement format needs
  an embed. Use the helper instead of adding a second posting path.

Write one notes file and use it for both the GitHub release and the
announcement. The helper turns each non-empty line into a `- ` bullet, so one
concrete user-facing change per line is the unit. Keep the file outside the
checkout: the release script rejects untracked files in the working tree.

```bash
node scripts/post_release_discord.mjs --version <X.Y.Z> \
  --notes <notes-file> \
  --link https://github.com/khentmoba/SpeakFlow/releases/download/<X.Y.Z>/SpeakFlow_<X.Y.Z>_x64-setup.exe \
  --configuration "Windows x64 (NSIS)"
```

Run it without `--send` first and read the printed payload: it must name the
version you published and a download URL that resolves to that release's
installer. Add `--send` to post.

The installer is around 28 MB, above Discord's 10 MB webhook upload limit, so
the announcement links to the published asset instead of attaching it. The
helper rejects an oversized `--file` with that explanation; switch to `--link`.

Post once. After an uncertain response, read `#speakflow-releases` before
retrying so a second announcement is not created. The announcement is authored
by the webhook, so the MCP bot cannot edit it; a correction means deleting the
post and posting again, which needs the maintainer's approval.

If the helper or its config is missing, finish the release and report the
announcement as blocked, with what is missing.

Done when the channel holds exactly one announcement for this version, and the
report below includes its message ID and link.

Report the release URL, version/tag, candidate SHA, CI result, feed
verification, and the Discord message ID and link.
