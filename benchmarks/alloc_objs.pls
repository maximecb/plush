let start_time = $time_current_ms();

for (let var i = 0; i < 2_000_000; i = i + 1)
{
    let o = { x: 1, y: 2 };
    i = i + 1;
}

let end_time = $time_current_ms();
$print_i64(end_time - start_time);
$print_endl();
