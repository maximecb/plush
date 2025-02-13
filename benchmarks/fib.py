def fib(n):
    if n < 2:
        return n

    return fib(n - 1) + fib(n - 2)

r = fib(38)
print(r)
