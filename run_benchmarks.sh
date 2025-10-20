#!/bin/bash

for f in benchmarks/*.psh; do
    echo "Running benchmark: $f"
    time cargo run --release "$f"
done
