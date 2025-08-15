let width = 800;
let height = 600;

// Draw a single pixel
fun draw(framebuffer, width, height, x, y, color) {
    if (x >= 0 && x < width && y >= 0 && y < height) {
        let index = y * width + x;
        framebuffer.write_u32(index, color);
    }
}

// Draw a line using Bresenham's line algorithm
fun draw_line(framebuffer, width, height, x0, y0, x1, y1, color) {
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = x0 < x1 ? 1 : -1;
    let sy = y0 < y1 ? 1 : -1;
    let var err = dx - dy;
    
    let var x = x0;
    let var y = y0;
    
    while (true) {
        // Draw current pixel
        draw(framebuffer, width, height, x, y, color);
        
        // Check if we've reached the end point
        if (x == x1 && y == y1) {
            break;
        }
        
        // Calculate error for next step
        let e2 = 2 * err;
        
        // Move in x direction
        if (e2 > -dy) {
            err = err - dy;
            x = x + sx;
        }
        
        // Move in y direction
        if (e2 < dx) {
            err = err + dx;
            y = y + sy;
        }
    }
}

let var framebuffer = ByteArray.with_size(width * height * 4);

// Clear to black
framebuffer.fill_u32(0, width * height, 0xFF000000);

draw_line(framebuffer, width, height, 100, 100, 700, 500, 0xFFFF0000); // Red diagonal line
draw_line(framebuffer, width, height, 50, 300, 750, 300, 0xFF00FF00);  // Green horizontal line
draw_line(framebuffer, width, height, 400, 50, 400, 550, 0xFF0000FF);  // Blue vertical line
draw_line(framebuffer, width, height, 100, 500, 700, 100, 0xFFFFFF00); // Yellow diagonal line

// Draw in a window
let window = $window_create(width, height, "Render", 0);
$window_draw_frame(window, framebuffer);

loop {
    let msg = $actor_recv();

    if (!(msg instanceof UIEvent)) {
        continue;
    }
    if (msg.kind == 'CLOSE_WINDOW' || (msg.kind == 'KEY_DOWN' && msg.key == 'ESCAPE')) {
        break;
    }
}
