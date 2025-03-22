fun fact(n)
{
    if (n <= 2)
        return n;

    return n * fact(n-1);
}

assert(fact(3) == 6);
