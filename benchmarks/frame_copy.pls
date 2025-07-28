let src = ByteArray.with_size(4 * 1024 * 1024);
let dst = ByteArray.with_size(4 * 1024 * 1024);

let num_frames = 20_000;

let start_time = $time_current_ms();

for (let var i = 0; i < num_frames; ++i)
{
    //$println(i.to_s());
    dst.copy_from(src, 0, 0, dst.len);
}

let end_time = $time_current_ms();
let dt = (end_time - start_time) / 1000;
let fps = num_frames / dt;
$println(fps.floor().to_s() + " fps");
