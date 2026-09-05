#!/usr/bin/env python3

# Compare plush against CPython on the fib benchmark.
# Runs the two interleaved N times and reports the median time for each.

import argparse
import os
import statistics
import subprocess
import sys
import time

NUM_RUNS = 5

def repo_root():
    return os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

def time_run(cmd):
    start = time.perf_counter()
    result = subprocess.run(cmd, capture_output=True, text=True)
    elapsed = time.perf_counter() - start

    if result.returncode != 0:
        print(f"command failed: {' '.join(cmd)}", file=sys.stderr)
        print(result.stderr, file=sys.stderr)
        sys.exit(1)

    return elapsed, result.stdout.strip()

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("-n", "--num-runs", type=int, default=NUM_RUNS,
                        help=f"number of runs per implementation (default: {NUM_RUNS})")
    parser.add_argument("--no-build", action="store_true",
                        help="skip the cargo build step")
    args = parser.parse_args()

    root = repo_root()
    os.chdir(root)

    if not args.no_build:
        print("Building plush")
        subprocess.run(["cargo", "build", "--release"], check=True)

    # Reports e.g. "CPython 3.14.6" (or PyPy etc. if python3 is not CPython)
    py_version = subprocess.run(
        ["python3", "-c", "import platform; print(platform.python_implementation(), platform.python_version())"],
        capture_output=True, text=True).stdout.strip()

    contenders = [
        ("plush (release)", ["target/release/plush", "benchmarks/fib.psh"]),
        (py_version, ["python3", "benchmarks/fib.py"]),
    ]

    times = {name: [] for name, _ in contenders}
    outputs = {}

    for run_idx in range(args.num_runs):
        print(f"\nRun {run_idx + 1}/{args.num_runs}")

        for name, cmd in contenders:
            elapsed, output = time_run(cmd)
            times[name].append(elapsed)
            outputs.setdefault(name, output)
            print(f"  {name}: {elapsed:.3f}s")

            if outputs[name] != output:
                print(f"  warning: {name} output changed between runs", file=sys.stderr)

    distinct = set(outputs.values())
    if len(distinct) > 1:
        print(f"\nwarning: implementations disagree: {outputs}", file=sys.stderr)

    median = {name: statistics.median(times[name]) for name, _ in contenders}

    print(f"\nMedian of {args.num_runs}:")
    for name, _ in contenders:
        print(f"  {name}: {median[name]:.3f}s")

    plush_time = median[contenders[0][0]]
    py_time = median[contenders[1][0]]

    if plush_time <= py_time:
        ratio = py_time / plush_time
        verdict = "faster"
    else:
        ratio = plush_time / py_time
        verdict = "slower"

    pct = (ratio - 1) * 100
    print(f"\nplush is {pct:.1f}% {verdict} than {py_version} ({ratio:.2f}x)")

main()
