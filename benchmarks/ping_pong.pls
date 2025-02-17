let var obj = {

    var count: 0,

    inc(self) {
        self.count = self.count + 1;
    }
};

// The actor receives the object and sends it back to the main actor
fun actor()
{
    while (true)
    {
        let the_obj = $actor_recv();
        the_obj.inc();
        $actor_send(0, the_obj);
    }
}

let id = $actor_spawn(actor);

for (let var i = 0; i < 500_000; i = i + 1)
{
    assert(obj.count == i);

    $actor_send(id, obj);
    obj = $actor_recv();

    assert(obj.count == i + 1);
}
