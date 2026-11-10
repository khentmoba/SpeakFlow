# Agent notes

- Before pushing, run `pnpm verify`. It runs the formal-contract, Python, frontend (including coverage thresholds), and Rust format/lint/test checks that CI runs. It does not run full TLC/Lean model checking or `pnpm audit`/`cargo audit`; CI runs those. Full list: `docs/development.md#checks`.
- A fresh checkout needs `pnpm install --frozen-lockfile` and `uv sync --frozen` before any check. This project's worktree scripts run both.
- On Windows, PowerShell 5.1 rejects `&&`. Run commands separately or join them with `;`.
- Frontend tests cover logic only: hooks via `renderHook`, `src/lib`, `src/api`, and config/contract checks. The project has no UI tests. Do not add tests that render components, query the DOM (`render`, `screen`, `fireEvent`, `userEvent`), check CSS, run accessibility scans, or benchmark rendering, and do not add UI-test libraries. When a UI change needs coverage, move the logic into a hook or `src/lib` function and test that.
- Pointers: release steps `docs/releasing.md`; formal models `formal/README.md`; CI definition `.github/workflows/ci.yml`.
- A PR performance number names the benchmark file, says whether this PR changed that file, and says whether the gain holds outside the measured action. For example, working-set trimming applies only while the window is hidden.
