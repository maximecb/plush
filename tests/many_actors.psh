let var g = 0;

fun f()
{
    return g + 1;
}

let ids = [];

for (let var i = 0; i < 100; i = i + 1)
{
    // The global variable g gets copied when the
    // actor spawns, so its current value is accessible
    ids.push($actor_spawn(f));
    g = g + 1;
}

let var sum = 0;

for (let var i = 0; i < ids.len; i = i + 1)
{
    sum = sum + $actor_join(ids[i]);
}

assert(sum == 5050);
$println(sum);
