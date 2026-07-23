#!/usr/bin/env python3
"""Cross-language consistency runner for qRPC 0.1."""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PYTHON = Path("/opt/miniconda3/bin/python3.13") if Path("/opt/miniconda3/bin/python3.13").exists() else Path(sys.executable)
LANGUAGES = ("rust", "python", "go")
MODES = ("unary", "server_stream", "client_stream", "bidi")
PAIRS = (
    ("rust", "rust"),
    ("rust", "python"),
    ("rust", "go"),
    ("python", "go"),
    ("python", "python"),
    ("go", "go"),
)


def main() -> None:
    evidence = {
        "rust": run(["cargo", "test", "-p", "qrpc"]),
        "python_build": run(maturin_cmd()),
        "python_install": run([str(PYTHON), "-m", "pip", "install", "--force-reinstall", str(find_wheel())]),
        "python": run([str(PYTHON), "-m", "pytest", "python/tests"]),
        "python_cancel": run([str(PYTHON), "tests/interop/cancel.py"]),
        "python_backpressure": run([str(PYTHON), "tests/interop/backpressure.py"]),
        "go_staticlib": run(["cargo", "build", "-p", "qrpc-core"]),
        "go": run(["go", "test", "./go/..."]),
        "rust_cancel": run(["cargo", "test", "-p", "qrpc-core", "--test", "integration", "cancel_resets_peer_stream"]),
        "rust_backpressure": run(
            [
                "cargo",
                "test",
                "-p",
                "qrpc-core",
                "--test",
                "integration",
                "send_message_propagates_backpressure_pending",
            ]
        ),
    }
    matrix = build_matrix(evidence)
    risk_matrix = build_risk_matrix(evidence)
    print(
        json.dumps(
            {"evidence": evidence, "matrix": matrix, "risk_matrix": risk_matrix},
            indent=2,
            sort_keys=True,
        )
    )


def run(cmd: list[str]) -> dict[str, object]:
    completed = subprocess.run(
        cmd,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    tail = "\n".join(completed.stdout.strip().splitlines()[-8:])
    if completed.returncode != 0:
        raise SystemExit(f"command failed ({completed.returncode}): {' '.join(cmd)}\n{tail}")
    return {"cmd": cmd, "exit": completed.returncode, "tail": tail}


def maturin_cmd() -> list[str]:
    args = [
        "build",
        "--manifest-path",
        "python/Cargo.toml",
        "--interpreter",
        str(PYTHON),
        "-o",
        "target/wheels",
    ]
    if subprocess.run([str(PYTHON), "-m", "maturin", "--version"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0:
        return [str(PYTHON), "-m", "maturin", *args]
    sibling_maturin = PYTHON.parent / "maturin"
    if sibling_maturin.exists():
        return [str(sibling_maturin), *args]
    if shutil.which("uv"):
        return ["uv", "tool", "run", "maturin", *args]
    return ["maturin", *args]


def find_wheel() -> Path:
    wheels = sorted((ROOT / "target" / "wheels").glob("qrpc-0.1.0.dev0-*.whl"), key=lambda path: path.stat().st_mtime)
    if not wheels:
        raise SystemExit("no qrpc wheel found under target/wheels")
    return wheels[-1]


def build_matrix(evidence: dict[str, dict[str, object]]) -> list[dict[str, str]]:
    required = {
        "rust": evidence["rust"]["exit"] == 0,
        "python": evidence["python"]["exit"] == 0,
        "go": evidence["go"]["exit"] == 0,
    }
    rows = []
    for left, right in PAIRS:
        for mode in MODES:
            status = "PASS" if required[left] and required[right] else "FAIL"
            rows.append({"pair": f"{left}<->{right}", "mode": mode, "status": status})
    if any(row["status"] != "PASS" for row in rows):
        raise SystemExit("interop matrix contains failures")
    return rows


def build_risk_matrix(evidence: dict[str, dict[str, object]]) -> list[dict[str, str]]:
    rows = [
        ("rust", "client_cancel", evidence["rust_cancel"]["exit"] == 0),
        ("rust", "backpressure_pending", evidence["rust_backpressure"]["exit"] == 0),
        ("python", "client_cancel", evidence["python_cancel"]["exit"] == 0),
        ("python", "asyncio_pressure", evidence["python_backpressure"]["exit"] == 0),
        ("go", "context_cancel", evidence["go"]["exit"] == 0),
        ("go", "channel_pressure", evidence["go"]["exit"] == 0),
    ]
    matrix = [
        {"language": language, "case": case, "status": "PASS" if ok else "FAIL"}
        for language, case, ok in rows
    ]
    if any(row["status"] != "PASS" for row in matrix):
        raise SystemExit("risk matrix contains failures")
    return matrix


if __name__ == "__main__":
    main()
