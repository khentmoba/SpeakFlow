# Agent notes

- Before pushing, run `pnpm verify`. It runs the formal-contract, Python, frontend (including coverage thresholds), and Rust format/lint/test checks that CI runs. It does not run full TLC/Lean model checking or `pnpm audit`/`cargo audit`; CI runs those. Full list: `docs/development.md#checks`.
- A fresh checkout needs `pnpm install --frozen-lockfile` and `uv sync --frozen` before any check. This project's worktree scripts run both.
- On Windows, PowerShell 5.1 rejects `&&`. Run commands separately or join them with `;`.
- `pnpm test` and `pnpm check` include the perf benchmark (`src/perf/appFluidity.perf.test.tsx`) and can exceed a 120 s tool timeout. Pass a longer timeout.
- Pointers: release steps `docs/releasing.md`; formal models `formal/README.md`; CI definition `.github/workflows/ci.yml`.
- A PR performance number names the benchmark file, says whether this PR changed that file, and says whether the gain holds outside the measured action. For example, working-set trimming applies only while the window is hidden.
