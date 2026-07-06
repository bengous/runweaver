#!/usr/bin/env python3
"""Measure runweaver hot-path overhead against bare process-spawn baselines.

Builds throwaway fixtures in a temp dir, runs each command 100 times after
10 warmup iterations, and prints median / mean / min / p95 wall times.

Usage:
    python3 scripts/bench-hot-path.py [path/to/runweaver-binary]

Defaults to target/release/runweaver relative to the repo root. Build it
first with `cargo build --release`.
"""

import json
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

WARMUP = 10
ITERATIONS = 100


def small_manifest() -> dict:
    return {
        "version": 2,
        "paths": {"writable": ["src/"]},
        "tools": {"echoCheck": {"script": "true"}},
        "pipelines": {"check": {"check": ["echoCheck"]}},
        "operations": {},
        "surfaces": {
            "agents": {
                "harnesses": ["claude", "codex"],
                "preTool": [{"guard": "destructive-commands"}],
                "stop": {"run": "check"},
            },
            "git": {"preCommit": {"run": "check"}},
            "cli": True,
        },
        "bindings": [],
    }


def big_manifest() -> dict:
    manifest = small_manifest()
    for i in range(100):
        manifest["tools"][f"tool{i}"] = {"script": "true"}
        manifest["pipelines"][f"pipeline{i}"] = {"check": [f"tool{i}"]}
    return manifest


def write_project(root: Path, manifest: dict) -> None:
    runweaver_dir = root / ".runweaver"
    runweaver_dir.mkdir(parents=True)
    (runweaver_dir / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")


def hook_payload(cwd: Path) -> bytes:
    return json.dumps(
        {
            "hook_event_name": "PreToolUse",
            "session_id": "bench-session",
            "transcript_path": "/tmp/transcript.jsonl",
            "cwd": str(cwd),
            "tool_use_id": "bench-tool-use",
            "tool_name": "Bash",
            "tool_input": {"command": "pwd"},
        }
    ).encode()


def bench(name: str, cmd: list[str], stdin: bytes = b"") -> float:
    for _ in range(WARMUP):
        result = subprocess.run(cmd, input=stdin, capture_output=True)
        if result.returncode != 0:
            print(
                f"FAIL {name}: rc={result.returncode} "
                f"stderr={result.stderr.decode()[:200]}",
                file=sys.stderr,
            )
            sys.exit(1)
    times = []
    for _ in range(ITERATIONS):
        start = time.perf_counter()
        subprocess.run(cmd, input=stdin, capture_output=True)
        times.append((time.perf_counter() - start) * 1000)
    median = statistics.median(times)
    p95 = sorted(times)[int(ITERATIONS * 0.95)]
    print(
        f"{name}: median={median:.2f}ms mean={statistics.mean(times):.2f}ms "
        f"min={min(times):.2f}ms p95={p95:.2f}ms"
    )
    return median


def main() -> None:
    repo_root = Path(__file__).resolve().parent.parent
    binary = (
        Path(sys.argv[1]).resolve()
        if len(sys.argv) > 1
        else repo_root / "target" / "release" / "runweaver"
    )
    if not binary.is_file():
        sys.exit(f"binary not found: {binary} (run `cargo build --release` first)")

    with tempfile.TemporaryDirectory(prefix="runweaver-bench-") as tmp:
        tmp_path = Path(tmp)
        proj = tmp_path / "proj"
        proj_big = tmp_path / "proj-big"
        write_project(proj, small_manifest())
        write_project(proj_big, big_manifest())

        baseline_true = bench("baseline /usr/bin/true", ["/usr/bin/true"])
        baseline_sh = bench("baseline sh -c true", ["/bin/sh", "-c", "true"])
        hook = bench(
            "hook dispatch (small manifest)",
            [str(binary), "hook", "claude", "guard-destructive", "--cwd", str(proj)],
            stdin=hook_payload(proj),
        )
        run_check = bench(
            "run check (small manifest)",
            [str(binary), "run", "check", "--cwd", str(proj)],
        )
        hook_big = bench(
            "hook dispatch (10x manifest)",
            [str(binary), "hook", "claude", "guard-destructive", "--cwd", str(proj_big)],
            stdin=hook_payload(proj_big),
        )
        git_hook = bench(
            "git-hook pre-commit (small manifest)",
            [str(binary), "git-hook", "pre-commit", "--cwd", str(proj)],
        )

    print()
    print(f"orchestrator overhead, hook dispatch:  {hook - baseline_true:.2f}ms")
    print(f"orchestrator overhead, run check:      {run_check - baseline_sh:.2f}ms")
    print(f"orchestrator overhead, git-hook:       {git_hook - baseline_sh:.2f}ms")
    print(f"manifest 10x load cost:                {hook_big - hook:+.2f}ms")


if __name__ == "__main__":
    main()
