let start_time = $time_current_ms();

class Foo
{
    init(self)
    {
        self.x = 1;
        self.y = 2;
    }
}

for (let var i = 0; i < 2_000_000; i = i + 1)
{
    let o = new Foo();
    i = i + 1;
}

let end_time = $time_current_ms();
$println(end_time - start_time);
