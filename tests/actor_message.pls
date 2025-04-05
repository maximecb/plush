// These classes represent two different kinds of messages
class Inc {}
class Get {}

let var g = 0;

// Event loop for a new actor
fun f()
{
    while (true)
    {
        let msg = $actor_recv();

        if (msg instanceof Inc)
            ++g;

        if (msg instanceof Get)
            $actor_send(0, g);
    }
}

let id = $actor_spawn(f);

for (let var i = 0; i < 10; ++i)
    $actor_send(id, new Inc());

$actor_send(id, new Get());
let cnt = $actor_recv();
$println(cnt);
assert(cnt == 10);
