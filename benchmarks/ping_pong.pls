// The actor receives the object and sends it back to the main actor
fun actor()
{
    while (true)
    {
        let f = $actor_recv();
        let r = f();
        assert(r == 777);
        $actor_send(0, f);
    }
}

let var f = fun()
{
    return 777;
};

let id = $actor_spawn(actor);

for (let var i = 0; i < 500_000; i = i + 1)
{
    $actor_send(id, f);
    f = $actor_recv();
}
