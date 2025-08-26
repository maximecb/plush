echo "Building plush"
cargo build --release

echo ""
echo "Plush latest (release)"
time target/release/plush benchmarks/fib.psh

echo ""
echo `python3 --version`
time python3 benchmarks/fib.py
