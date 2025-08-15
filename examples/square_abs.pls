fun max(a, b) {
    if (a > b) return a;
    return b;
}

fun min(a, b) {
    if (a < b) return a;
    return b;
}

class Vector2 {
    init(self, x, y) {
        self.x = x;
        self.y = y;
    }
}

// Rasterize a triangle into a ByteArray framebuffer
fun rasterize_triangle(framebuffer, width, height, v0, v1, v2) {
    // Convert vertices to integer coordinates
    let x0 = (v0.x * width).floor();
    let y0 = (v0.y * height).floor();
    let x1 = (v1.x * width).floor();
    let y1 = (v1.y * height).floor();
    let x2 = (v2.x * width).floor();
    let y2 = (v2.y * height).floor();

    // Compute bounding box
    let minX = max(0, min(x0, min(x1, x2)));
    let maxX = min(width - 1, max(x0, max(x1, x2)));
    let minY = max(0, min(y0, min(y1, y2)));
    let maxY = min(height - 1, max(y0, max(y1, y2)));

    // Precompute barycentric coordinate divisors
    let var area = (y1 - y2) * (x0 - x2) + (x2 - x1) * (y0 - y2);
    area = area.abs();
    if (area == 0) return; // Degenerate triangle

    // Scan through bounding box
    for (let var y = minY; y <= maxY; y = y + 1) {
        for (let var x = minX; x <= maxX; x = x + 1) {
            // Compute barycentric coordinates
            let w0 = ((y1 - y2) * (x - x2) + (x2 - x1) * (y - y2)).abs();
            let w1 = ((y2 - y0) * (x - x2) + (x0 - x2) * (y - y2)).abs();
            let w2 = area - w0 - w1;

            // Check if point is inside triangle
            if ((w0 >= 0 && w1 >= 0 && w2 >= 0) || (w0 <= 0 && w1 <= 0 && w2 <= 0)) {
                // Normalize barycentric coordinates to [0,1]
                let f0 = w0 / area;
                let f1 = w1 / area;
                let f2 = w2 / area;

                let r = (f0 * 255).floor();
                let g = (f1 * 255).floor();
                let b = (f2 * 255).floor();
                let rgb = (0xFF << 24) | (r << 16) | (g << 8) | b;
                let index = y * width + x;
                framebuffer.write_u32(index, rgb);
            }
        }
    }
}

let width = 600;
let height = 600;
let framebuffer = ByteArray.with_size(width * height * 4);

// Clear to black
framebuffer.fill_u32(0, width * height, 0xFF000000);

// Four corners of a square
let a = Vector2(0.1, 0.1); // Top-left
let b = Vector2(0.9, 0.9); // Bottom-right
let c = Vector2(0.1, 0.9); // Bottom-left
let d = Vector2(0.9, 0.1); // Top-right

// Measure rasterization time
let var start_time = $time_current_ms();

// Rasterize triangle
rasterize_triangle(framebuffer, width, height, a, b, c);

// Display elapsed time
let var end_time = $time_current_ms() - start_time;
$println("Rasterization time: " + end_time.to_s() + "ms");

// Create a window
let window = $window_create(width, height, "Triangle Rasterizer", 0);

// Draw in a window
$window_draw_frame(window, framebuffer);

// Measure rasterization time
start_time = $time_current_ms();

// Rasterize triangle
rasterize_triangle(framebuffer, width, height, a, b, d);

// Display elapsed time
end_time = $time_current_ms() - start_time;
$println("Rasterization time: " + end_time.to_s() + "ms");

// Draw in a window
$window_draw_frame(window, framebuffer);

loop {
    let msg = $actor_recv();
    if (msg instanceof UIEvent) {
        if (msg.kind == 'CLOSE_WINDOW' || (msg.kind == 'KEY_DOWN' && msg.key == 'ESCAPE')) {
            break;
        }
    }
}
