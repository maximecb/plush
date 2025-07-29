echo "Building plush"
cargo build --release

echo ""
echo "Plush latest (release)"
time target/release/plush benchmarks/fib.pls

echo ""
echo `python3 --version`
time python3 benchmarks/fib.py
