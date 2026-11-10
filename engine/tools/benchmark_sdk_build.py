#!/usr/bin/env python3
"""Measure SDK archive builds separately from TinyCC query compile and link."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import statistics
import subprocess
import tempfile
import time


ROOT = Path(__file__).resolve().parents[2]
MAKEFILE = ROOT / "engine/sdk/Makefile"
QUERY_SOURCE = ROOT / "engine/tests/fixtures/sql_v3_caught_error.c"


def measure(command: list[str], cwd: Path) -> float:
    start = time.perf_counter()
    subprocess.run(
        command,
        cwd=cwd,
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
    )
    return (time.perf_counter() - start) * 1000.0


def summarize(samples: list[float]) -> dict[str, float | int]:
    return {
        "samples_ms": [round(sample, 3) for sample in samples],
        "median_ms": round(statistics.median(samples), 3),
        "min_ms": round(min(samples), 3),
        "max_ms": round(max(samples), 3),
    }


def first_and_warm(samples: list[float]) -> dict[str, object]:
    return {
        "first_sample_ms": round(samples[0], 3),
        "subsequent_samples": summarize(samples[1:]) if len(samples) > 1 else None,
    }


def tool_version(tool: str, argument: str) -> str:
    result = subprocess.run(
        [tool, argument],
        check=False,
        capture_output=True,
        text=True,
    )
    output = (result.stdout + result.stderr).strip().splitlines()
    return output[0] if output else f"exit {result.returncode}"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--runs", type=int, default=5,
        help="clean archive and per-query samples (default: 5; minimum: 2)",
    )
    args = parser.parse_args()
    if args.runs < 2 or args.runs > 50:
        parser.error("--runs must be between 2 and 50")

    make = shutil.which("make")
    tcc = shutil.which("tcc")
    linker = shutil.which("cc")
    if not make or not tcc or not linker:
        parser.error("make, TinyCC, and cc are required; use the pinned Nix shell")

    clean_builds: list[float] = []
    query_compiles: list[float] = []
    query_links: list[float] = []
    with tempfile.TemporaryDirectory(prefix="prdb-sdk-build-bench-") as temp:
        bench_root = Path(temp)
        first_build = bench_root / "sdk-0"
        archive = first_build / "libprdb_sdk.a"
        archive_command = [
            make,
            "-f",
            str(MAKEFILE),
            f"BUILD_DIR={first_build}",
            str(archive),
        ]
        for index in range(args.runs):
            build_dir = bench_root / f"sdk-{index}"
            build_dir.mkdir()
            command = [
                make,
                "-f",
                str(MAKEFILE),
                f"BUILD_DIR={build_dir}",
                str(build_dir / "libprdb_sdk.a"),
            ]
            clean_builds.append(measure(command, ROOT))

        warm_archive_build = measure(archive_command, ROOT)

        for index in range(args.runs):
            query_dir = bench_root / f"query-{index}"
            query_dir.mkdir()
            obj = query_dir / "query.o"
            so = query_dir / "query.so"
            query_compiles.append(
                measure(
                    [
                        tcc,
                        "-Wall",
                        "-Wextra",
                        "-Werror",
                        "-I",
                        str(ROOT / "engine/include"),
                        "-c",
                        str(QUERY_SOURCE),
                        "-o",
                        str(obj),
                    ],
                    ROOT,
                )
            )
            query_links.append(
                measure(
                    [
                        linker,
                        "-shared",
                        "-Wl,--no-eh-frame-hdr,-z,noexecstack",
                        str(obj),
                        str(archive),
                        "-o",
                        str(so),
                    ],
                    ROOT,
                )
            )

    print(
        json.dumps(
            {
                "profile": {
                    "platform": os.uname().sysname + "-" + os.uname().machine,
                    "tcc": tool_version(tcc, "-version"),
                    "linker": tool_version(linker, "--version"),
                    "runs": args.runs,
                    "query_source": QUERY_SOURCE.relative_to(ROOT).as_posix(),
                    "os_cache_state": "uncontrolled; first sample is not guaranteed cold",
                },
                "sdk_archive_clean_build": summarize(clean_builds),
                "sdk_archive_noop_rebuild_ms": round(warm_archive_build, 3),
                "query_compile_tinycc": first_and_warm(query_compiles),
                "query_shared_link_gnu": first_and_warm(query_links),
            },
            indent=2,
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
