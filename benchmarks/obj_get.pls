class Foo
{
    init(self)
    {
        self.x = 1;
    }
}

let o = new Foo();

for (let var i = 0; i < 100_000_000; i = i + 1)
{
    o.x;
}
