fun fib(n)
{
    if (n < 2)
        return n;

    return fib(n - 1) + fib(n - 2);
}

let r = fib(38);
$println(r);
