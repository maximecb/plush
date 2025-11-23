#!/bin/bash

# Fail immediately if a benchmark fails
set -euo pipefail

for f in benchmarks/*.psh; do
    echo "Running benchmark: $f"
    time cargo run --release "$f"
done
