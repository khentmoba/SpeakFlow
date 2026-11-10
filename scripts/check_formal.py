"""Run the checked application contracts; model checks are not source proofs."""

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
from concurrent.futures import FIRST_EXCEPTION, ThreadPoolExecutor, wait
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TLC_SHA256 = "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"
MAX_RAM_PERCENT = 25


def tla_code(source: str) -> str:
    """Keep code/line structure, excluding comments and literal contents."""
    code, depth, index = [], 0, 0
    while index < len(source):
        pair = source[index:index + 2]
        if pair == "(*":
            depth += 1
            code.append("  ")
            index += 2
        elif depth and pair == "*)":
            depth -= 1
            code.append("  ")
            index += 2
        elif depth:
            code.append("\n" if source[index] == "\n" else " ")
            index += 1
        elif pair == r"\*":
            end = source.find("\n", index)
            end = len(source) if end == -1 else end
            code.append(" " * (end - index))
            index = end
        elif source[index] == '"':
            index += 1
            while index < len(source) and source[index] != '"':
                index += 2 if source[index] == "\\" else 1
            if index >= len(source):
                raise ValueError("unterminated TLA string")
            code.append('""')
            index += 1
        else:
            code.append(source[index])
            index += 1
    if depth:
        raise ValueError("unterminated TLA comment")
    return re.sub(r"^=+\s*$", "", "".join(code), flags=re.M)


def event_inventory() -> set[str]:
    names = set()
    patterns = [
        r'\.emit\(\s*"([\w-]+)"',
        r'(?:listen|useTauriEvent)(?:<[^;]{0,200}?>)?\(\s*"([\w-]+)"',
        r'(?:const|static)\s+\w*EVENT\w*\s*[^=]*=\s*"([\w-]+)"',
    ]
    for folder in ["src-tauri/src", "src"]:
        for path in (ROOT / folder).rglob("*"):
            if path.suffix not in {".rs", ".ts", ".tsx"} or "test" in str(path.relative_to(ROOT)).lower():
                continue
            source = path.read_text(encoding="utf-8")
            for pattern in patterns:
                names.update(re.findall(pattern, source))
    return names


def check_inventory(manifest: dict) -> None:
    source = (ROOT / "src-tauri/src/lib.rs").read_text(encoding="utf-8")
    handler = source.split("tauri::generate_handler![", 1)[1].split("]", 1)[0]
    actual = set(re.findall(r"commands::\w+::\w+", handler))
    expected = set(manifest["commands"])
    if actual != expected:
        raise ValueError(f"command inventory differs: missing={actual - expected}, removed={expected - actual}")
    if set(manifest["command_implementation"]) != actual:
        raise ValueError("each command requires its exact implementation anchor")
    for anchor in manifest["command_implementation"].values():
        path, _, symbol = anchor.partition("#")
        if symbol not in (ROOT / path).read_text(encoding="utf-8"):
            raise ValueError(f"command implementation anchor missing: {anchor}")
    if event_inventory() != set(manifest["events"]):
        raise ValueError(f"event inventory differs: {event_inventory() ^ set(manifest['events'])}")
    for kind in ["commands", "events", "lifecycle", "protocols"]:
        for name, bindings in manifest[kind].items():
            if not bindings:
                raise ValueError(f"unbound {kind}: {name}")
            for binding in bindings:
                contract = manifest["contracts"][binding]
                for required in ["obligation", "models", "implementation", "evidence", "assumptions"]:
                    if not contract.get(required):
                        raise ValueError(f"missing {required}: {name}/{binding}")
    for contract in manifest["contracts"].values():
        for implementation in contract["implementation"] + contract["evidence"]:
            path, _, symbol = implementation.partition("#")
            content = (ROOT / path).read_text(encoding="utf-8")
            if symbol and symbol not in content:
                raise ValueError(f"implementation/evidence anchor missing: {implementation}")
        for model, operators in contract["models"].items():
            content = (ROOT / "formal" / f"{model}.tla").read_text(encoding="utf-8")
            for operator in operators:
                if not re.search(rf"^{re.escape(operator)}(?:\([^\n]*\))?\s*==", content, re.M):
                    raise ValueError(f"formal obligation anchor missing: {model}.{operator}")
    for case in manifest["negative"]:
        if "replace" in case:
            source = (ROOT / "formal" / f"{case['model']}.tla").read_text(encoding="utf-8")
            if source.count(case["replace"][0]) != 1:
                raise ValueError(f"mutation target changed: {case['name']}")
    for model in manifest.get("runs", {}):
        check_static_model_bindings(manifest, model)
    print(f"Inventory: {len(actual)} commands, {len(manifest['events'])} event families, "
          f"{len(manifest['lifecycle'])} lifecycle contracts, {len(manifest['protocols'])} protocols.", flush=True)


def tlc(jar: Path, model: str, config: Path, source: Path, directory: Path,
        expected_violation: str | None = None, workers: int = 2) -> dict:
    directory.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="speak-flow-tlc-") as scratch:
        result = subprocess.run([
            "java", "-XX:+UseParallelGC", f"-XX:MaxRAMPercentage={MAX_RAM_PERCENT}",
            # TLC unpacks standard modules (e.g. Naturals.tla) into java.io.tmpdir
            # under fixed names; sharing the dir across parallel runs corrupts them.
            f"-Djava.io.tmpdir={scratch}",
            "-jar", str(jar), "-workers", str(workers),
            "-coverage", "1",
            "-metadir", str(Path(scratch) / "states"), "-config", str(config), str(source),
        ], cwd=source.parent, text=True, encoding="utf-8", errors="replace", capture_output=True, timeout=480)
    (directory / "tlc.log").write_text(result.stdout + result.stderr, encoding="utf-8")
    if expected_violation:
        if result.returncode != 12 or f"Invariant {expected_violation} is violated" not in result.stdout:
            raise ValueError(f"negative check did not expose {expected_violation}: {directory / 'tlc.log'}")
    elif result.returncode or "Model checking completed. No error has been found." not in result.stdout:
        raise ValueError(f"TLC did not pass: {directory / 'tlc.log'}")
    counts = re.findall(r"([\d,]+) states generated, ([\d,]+) distinct states found", result.stdout)
    count = int(counts[-1][1].replace(",", "")) if counts else None
    print(f"{'NEGATIVE' if expected_violation else 'PASS'} {model}/{config.name}: {count} distinct states", flush=True)
    record = {"model": model, "config": config.name, "distinct": count,
              "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
              "config_sha256": hashlib.sha256(config.read_bytes()).hexdigest(),
              "tlc_sha256": TLC_SHA256,
              "expected_violation": expected_violation}
    (directory / "result.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    return record


def run_all(tasks: list, jobs: int) -> list:
    """Run tasks concurrently, keep their order, and stop scheduling after the first failure."""
    with ThreadPoolExecutor(max_workers=jobs) as pool:
        futures = [pool.submit(task) for task in tasks]
        done, _ = wait(futures, return_when=FIRST_EXCEPTION)
        failed = next((future for future in done if future.exception()), None)
        if failed:
            pool.shutdown(cancel_futures=True)
            failed.result()
        return [future.result() for future in futures]


def reusable(record_path: Path, source: Path, config: Path) -> dict | None:
    try:
        record = json.loads(record_path.read_text(encoding="utf-8"))
        output = (record_path.parent / "tlc.log").read_text(encoding="utf-8")
    except (OSError, ValueError):
        return None
    if (isinstance(record, dict)
            and record.get("tlc_sha256") == TLC_SHA256
            and record.get("expected_violation") is None
            and record.get("source_sha256") == hashlib.sha256(source.read_bytes()).hexdigest()
            and record.get("config_sha256") == hashlib.sha256(config.read_bytes()).hexdigest()
            and "Model checking completed. No error has been found." in output):
        return record
    return None


def positive_run(jar: Path, model: str, config: str, logs: Path, reuse: bool) -> dict:
    source = ROOT / "formal" / f"{model}.tla"
    cfg = ROOT / "formal" / config
    directory = logs / Path(config).stem
    record = reusable(directory / "result.json", source, cfg) if reuse else None
    if record:
        print(f"REUSE verified {model}/{config}: {record['distinct']} distinct states", flush=True)
        return record
    return tlc(jar, model, cfg, source, directory)


def negative_run(jar: Path, case: dict, logs: Path) -> dict:
    model = case["model"]
    source = ROOT / "formal" / f"{model}.tla"
    config = ROOT / "formal" / case.get("config", f"{model}.cfg")
    with tempfile.TemporaryDirectory(prefix="speak-flow-negative-") as temporary:
        if "replace" in case:
            original, replacement = case["replace"]
            content = source.read_text(encoding="utf-8")
            if content.count(original) != 1:
                raise ValueError(f"mutation target changed: {case['name']}")
            source = Path(temporary) / f"{model}.tla"
            source.write_text(content.replace(original, replacement), encoding="utf-8")
            # TLC 1.7.4 resolves configs beside the module, including mutations.
            local_config = Path(temporary) / config.name
            local_config.write_bytes(config.read_bytes())
            config = local_config
        return tlc(jar, model, config, source, logs / f"negative-{case['name']}", case["invariant"])


def check_static_model_bindings(manifest: dict, model: str) -> tuple[set[str], set[str], set[str]]:
    source = tla_code((ROOT / "formal" / f"{model}.tla").read_text(encoding="utf-8"))
    declarations = list(re.finditer(r"^(\w+)(?:\([^\n]*\))?\s*==", source, re.M))
    bodies = {match[1]: source[match.end():declarations[index + 1].start() if index + 1 < len(declarations) else len(source)]
              for index, match in enumerate(declarations)}
    checked = set()
    for config in manifest["runs"][model]:
        checked.update(re.findall(r"^(?:INVARIANT|PROPERTY)\s+(\w+)",
                                  tla_code((ROOT / "formal" / config).read_text(encoding="utf-8")), re.M))
    # Only a positive conjunction or alias entails its operands. Merely naming
    # a predicate in a negation, implication, string or comment proves nothing.
    while True:
        closure = set(checked)
        for name in checked:
            body = bodies[name].strip()
            if re.fullmatch(r"(?:/\\\s*)?\w+(?:\s*/\\\s*\w+)*", body):
                closure.update(word for word in re.findall(r"\w+", body) if word in bodies)
        if closure == checked:
            break
        checked = closure
    bound = {operator for contract in manifest["contracts"].values()
             for operator in contract["models"].get(model, [])}
    actions = set(manifest.get("actions", {}).get(model, []))
    for action in actions:
        if action not in bodies or not re.search(r"\b\w+'", bodies[action]):
            raise ValueError(f"declared action has no state transition: {model}.{action}")
    missing = bound - actions - checked
    if missing:
        raise ValueError(f"unexercised/unasserted obligations in {model}: {sorted(missing)}")
    return checked, bound, actions


def check_model_bindings(manifest: dict, model: str, logs: Path) -> None:
    checked, bound, actions = check_static_model_bindings(manifest, model)
    observed = set()
    for config in manifest["runs"][model]:
        output = (logs / Path(config).stem / "tlc.log").read_text(encoding="utf-8")
        observed.update(name for name, count in re.findall(
            r"^<(\w+) line [^\n]+>: \d+:(\d+)$", output, re.M) if int(count) > 0)
    missing = (bound - actions - checked) | ((bound & actions) - observed)
    if missing:
        raise ValueError(f"unexercised/unasserted obligations in {model}: {sorted(missing)}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--inventory-only", action="store_true")
    parser.add_argument("--tlc-jar", type=Path)
    parser.add_argument("--lean", type=Path)
    parser.add_argument("--logs", type=Path)
    parser.add_argument("--negative-only", action="store_true")
    parser.add_argument("--reuse-completed", action="store_true",
                        help="Reuse only successful logs with matching source/config/TLC hashes")
    parser.add_argument("--jobs", type=int, default=max(1, (os.cpu_count() or 2) // 2),
                        help="TLC runs in parallel; each uses two workers")
    args = parser.parse_args()
    global MAX_RAM_PERCENT
    # Parallel JVMs share the runner's memory; JVM default is 25% each.
    MAX_RAM_PERCENT = max(10, min(25, 75 // max(1, args.jobs)))
    try:
        manifest = json.loads((ROOT / "formal/verification.json").read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        raise SystemExit(f"cannot read formal/verification.json: {error}") from error
    check_inventory(manifest)
    traces = subprocess.run([sys.executable, "scripts/generate_async_ownership_traces.py", "--check"],
                            cwd=ROOT, text=True, capture_output=True, timeout=30)
    if traces.returncode:
        raise ValueError(traces.stdout + traces.stderr)
    print(traces.stdout.strip(), flush=True)
    snapshot = subprocess.run([sys.executable, "scripts/check_request_snapshot.py"], cwd=ROOT,
                              text=True, capture_output=True, timeout=30)
    if snapshot.returncode:
        raise ValueError(snapshot.stdout + snapshot.stderr)
    print(snapshot.stdout.strip(), flush=True)
    if args.inventory_only:
        return 0
    if not args.tlc_jar or not args.lean or not args.logs:
        parser.error("full check requires --tlc-jar, --lean and --logs")
    jar = args.tlc_jar.resolve()
    if hashlib.sha256(jar.read_bytes()).hexdigest() != TLC_SHA256:
        raise ValueError("unrecognized TLC binary hash")
    logs = args.logs.resolve()
    logs.mkdir(parents=True, exist_ok=True)
    results = []
    if not args.negative_only:
        runs = [(model, config) for model, configs in manifest["runs"].items() for config in configs]
        results += run_all([
            (lambda model=model, config=config:
                positive_run(jar, model, config, logs, args.reuse_completed))
            for model, config in runs
        ], args.jobs)
        for model in manifest["runs"]:
            check_model_bindings(manifest, model, logs)
        coverage = subprocess.run([
            sys.executable, "scripts/check_tla_action_coverage.py",
            *[str(logs / Path(config).stem / "tlc.log") for config in manifest["runs"]["AppWorkflow"]],
        ], cwd=ROOT, text=True, capture_output=True, timeout=30)
        if coverage.returncode:
            raise ValueError(coverage.stdout + coverage.stderr)
        print(coverage.stdout.strip(), flush=True)
        lean_source = ROOT / "formal/lean/StateContracts.lean"
        if re.search(r"\b(?:sorry|admit)\b|^\s*axiom\b", lean_source.read_text(encoding="utf-8"), re.M):
            raise ValueError("unproved Lean declaration in application contracts")
        lean = subprocess.run([str(args.lean.resolve()), str(lean_source)], cwd=ROOT,
                              text=True, capture_output=True, timeout=120)
        (logs / "lean.log").write_text(lean.stdout + lean.stderr, encoding="utf-8")
        if lean.returncode:
            raise ValueError(f"Lean did not pass: {logs / 'lean.log'}")
        print("PASS Lean StateContracts", flush=True)
    results += run_all([
        (lambda case=case: negative_run(jar, case, logs)) for case in manifest["negative"]
    ], args.jobs)
    summary = "negative-summary.json" if args.negative_only else "summary.json"
    (logs / summary).write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
