fun fib(n)
{
    if (n < 2)
        return n;

    return fib(n - 1) + fib(n - 2);
}

let r = fib(28);
assert(r == 317811);
$print_i64(r);
$print_endl();
