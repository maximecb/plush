fun fact(n)
{
    if (n <= 2)
        return n;

    return n * fact(n-1);
}

//assert(fact(1) == 1);
//assert(fact(2) == 2);
assert(fact(3) == 6);
