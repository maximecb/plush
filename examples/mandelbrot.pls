// Mandelbrot Set Explorer

// This example renders the Mandelbrot fractal, showcasing floating-point
// math, complex calculations, and graphical rendering.

let WIDTH = 640;
let HEIGHT = 480;

// The region of the complex plane to render
let RE_START = -2.0;
let RE_END = 1.0;
let IM_START = -1.0;
let IM_END = 1.0;

let MAX_ITER = 60;

// Helper function to convert RGB values to a 32-bit color
fun rgb32(r, g, b) {
    return 0xFF_00_00_00 | (r << 16) | (g << 8) | b;
}

// Create a simple color palette
let palette = [];
for (let var i = 0; i < MAX_ITER; ++i) {
    let t = i / MAX_ITER;
    let r = (9 * (1 - t) * t * t * t * 255).floor();
    let g = (15 * (1 - t) * (1 - t) * t * t * 255).floor();
    let b = (8.5 * (1 - t) * (1 - t) * (1 - t) * t * 255).floor();
    palette.push(rgb32(r, g, b));
}

// --- Main Rendering Logic ---

let frame_buffer = ByteArray.with_size(WIDTH * HEIGHT * 4);

for (let var y = 0; y < HEIGHT; ++y) {
    for (let var x = 0; x < WIDTH; ++x) {
        // Map the pixel to a point in the complex plane
        let c_re = RE_START + (x / WIDTH) * (RE_END - RE_START);
        let c_im = IM_START + (y / HEIGHT) * (IM_END - IM_START);

        let var z_re = 0.0;
        let var z_im = 0.0;
        let var iter = 0;

        // Check if the point is in the Mandelbrot set
        while (z_re * z_re + z_im * z_im <= 4.0 && iter < MAX_ITER) {
            let z_re_new = z_re * z_re - z_im * z_im + c_re;
            z_im = 2.0 * z_re * z_im + c_im;
            z_re = z_re_new;
            iter = iter + 1;
        }

        // Set the pixel color based on the number of iterations
        let color = (iter == MAX_ITER) ? rgb32(0, 0, 0) : palette[iter];
        let idx = y * WIDTH + x;
        frame_buffer.write_u32(idx, color);
    }
}

// --- Window and Event Loop ---

let window = $window_create(WIDTH, HEIGHT, "Mandelbrot Set", 0);
$window_draw_frame(window, frame_buffer);

loop {
    let msg = $actor_recv();

    if (!(msg instanceof UIEvent)) {
        continue;
    }
    if (msg.kind == 'CLOSE_WINDOW' || (msg.kind == 'KEY_DOWN' && msg.key == 'ESCAPE')) {
        break;
    }
}
