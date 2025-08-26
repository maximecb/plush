// Bouncing Ball Example

// This example demonstrates simple graphics and animation by making a ball
// bounce around the screen.

let WIDTH = 640;
let HEIGHT = 480;

// Helper function to convert RGB values to a 32-bit color
fun rgb32(r, g, b) {
    return 0xFF_00_00_00 | (r << 16) | (g << 8) | b;
}

let BLACK = rgb32(0, 0, 0);
let BLUE = rgb32(50, 100, 255);

// --- Ball State ---
let var ball_x = 100.0;
let var ball_y = 100.0;
let var ball_vx = 2.5;
let var ball_vy = 3.5;
let var ball_radius = 20;

// --- Graphics Functions ---

// Draws a filled circle in a ByteArray
fun draw_circle(buffer, cx, cy, radius, color) {
    let cx_i = cx.floor();
    let cy_i = cy.floor();

    for (let var y = cy_i - radius; y < cy_i + radius; ++y) {
        for (let var x = cx_i - radius; x < cx_i + radius; ++x) {
            // Ensure we are within the window bounds
            if (x >= 0 && x < WIDTH && y >= 0 && y < HEIGHT) {
                let dx = x - cx_i;
                let dy = y - cy_i;
                // Check if the pixel is inside the circle's radius
                if (dx*dx + dy*dy < radius*radius) {
                    let idx = y * WIDTH + x;
                    buffer.write_u32(idx, color);
                }
            }
        }
    }
}

// --- Main Program ---

let frame_buffer = ByteArray.with_size(WIDTH * HEIGHT * 4);
let window = $window_create(WIDTH, HEIGHT, "Bouncing Ball", 0);

loop {
    // --- Update ball position ---
    ball_x = ball_x + ball_vx;
    ball_y = ball_y + ball_vy;

    // --- Collision detection (bouncing) ---
    if (ball_x - ball_radius < 0 || ball_x + ball_radius > WIDTH) {
        ball_vx = -ball_vx;
    }
    if (ball_y - ball_radius < 0 || ball_y + ball_radius > HEIGHT) {
        ball_vy = -ball_vy;
    }

    // --- Drawing ---
    frame_buffer.zero_fill();
    draw_circle(frame_buffer, ball_x, ball_y, ball_radius, BLUE);
    $window_draw_frame(window, frame_buffer);

    // Limit the frame rate
    $actor_sleep(10);

    // --- Event Handling ---
    let msg = $actor_poll();

    if (msg == nil) {
        continue;
    }
    if (!(msg instanceof UIEvent)) {
        continue;
    }
    if (msg.kind == 'CLOSE_WINDOW' || (msg.kind == 'KEY_DOWN' && msg.key == 'ESCAPE')) {
        break;
    }
}
