fun fib(n)
{
    if (n < 2)
        return n;

    return fib(n - 1) + fib(n - 2);
}

let r = fib(28);
assert(r == 317811);
$println(r);
