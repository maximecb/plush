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

// Optimized triangle rasterization with minimal calculations
fun rasterize_triangle(framebuffer, width, height, v0, v1, v2) {
    // Convert vertices to integer coordinates
    let x0 = (v0.x * width).floor();
    let y0 = (v0.y * height).floor();
    let x1 = (v1.x * width).floor();
    let y1 = (v1.y * height).floor();
    let x2 = (v2.x * width).floor();
    let y2 = (v2.y * height).floor();

    // Compute bounding box with inlined min/max
    let minX_temp = x0 < x1 ? x0 : x1;
    let minX = (minX_temp < x2 ? minX_temp : x2) < 0 ? 0 : (minX_temp < x2 ? minX_temp : x2);
    let maxX_temp = x0 > x1 ? x0 : x1;
    let maxX = (maxX_temp > x2 ? maxX_temp : x2) > width - 1 ? width - 1 : (maxX_temp > x2 ? maxX_temp : x2);
    let minY_temp = y0 < y1 ? y0 : y1;
    let minY = (minY_temp < y2 ? minY_temp : y2) < 0 ? 0 : (minY_temp < y2 ? minY_temp : y2);
    let maxY_temp = y0 > y1 ? y0 : y1;
    let maxY = (maxY_temp > y2 ? maxY_temp : y2) > height - 1 ? height - 1 : (maxY_temp > y2 ? maxY_temp : y2);

    // Precompute barycentric coordinate constants
    let area = (y1 - y2) * (x0 - x2) + (x2 - x1) * (y0 - y2);
    if (area == 0) return; // Degenerate triangle
    
    // Precompute edge function coefficients for incremental calculation
    let A01 = y0 - y1;
    let B01 = x1 - x0;
    let A12 = y1 - y2;
    let B12 = x2 - x1;
    let A20 = y2 - y0;
    let B20 = x0 - x2;
    
    // Precompute area reciprocal to avoid division in inner loop
    let inv_area = 1.0 / area;
    let inv_area_255 = 255.0 / area;

    // Scan through bounding box with optimized barycentric calculation
    for (let var y = minY; y <= maxY; y = y + 1) {
        // Calculate barycentric coordinates at start of row
        let var w0_row = A12 * (minX - x2) + B12 * (y - y2);
        let var w1_row = A20 * (minX - x2) + B20 * (y - y2);
        let var pixel_index = y * width + minX;
        
        for (let var x = minX; x <= maxX; x = x + 1) {
            // Use incremental barycentric coordinates
            let w2 = area - w0_row - w1_row;

            // Check if point is inside triangle (only compute w2 if needed)
            if (w0_row >= 0 && w1_row >= 0 && w2 >= 0) {
                let r = (w0_row * inv_area_255).floor();
                let g = (w1_row * inv_area_255).floor();
                let b = (w2 * inv_area_255).floor();
                let rgb = (0xFF << 24) | (r << 16) | (g << 8) | b;
                framebuffer.write_u32(pixel_index, rgb);
            }
            
            // Increment barycentric coordinates and pixel index
            w0_row = w0_row + A12;
            w1_row = w1_row + A20;
            pixel_index = pixel_index + 1;
        }
    }
}

let width = 400;
let height = 400;
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

// Display in a window
let window = $window_create(width, height, "Triangle Rasterizer", 0);
$window_draw_frame(window, framebuffer);

// Measure rasterization time
start_time = $time_current_ms();
rasterize_triangle(framebuffer, width, height, a, d, b);

// Display elapsed time
end_time = $time_current_ms() - start_time;
$println("Rasterization time: " + end_time.to_s() + "ms");

// Display in a window
$window_draw_frame(window, framebuffer);

loop {
    let msg = $actor_recv();
    if (msg instanceof UIEvent) {
        if (msg.kind == 'CLOSE_WINDOW' || (msg.kind == 'KEY_DOWN' && msg.key == 'ESCAPE')) {
            break;
        }
    }
}


    