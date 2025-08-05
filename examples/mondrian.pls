// Generates abstract art in the style of Piet Mondrian.
//
// The algorithm works by recursively subdividing the canvas. It starts with a
// single white rectangle and then randomly splits it either horizontally or
// vertically with a thick black line. This process is repeated for the new,
// smaller rectangles. When a rectangle is not subdivided further, it is
// filled with a primary color (red, yellow, blue) or, most commonly, white.

let WIDTH = 600;
let HEIGHT = 600;
let MIN_SIZE = 40;
let LINE_WIDTH = 12;
let MIN_DEPTH = 1;

// Helper function to convert RGB values to a 32-bit color
fun rgb32(r, g, b) {
    return 0xFF_00_00_00 | (r << 16) | (g << 8) | b;
}

// Define the color palette
let BLACK = rgb32(0, 0, 0);
let WHITE = rgb32(244, 244, 244);
let RED = rgb32(221, 1, 0);
let YELLOW = rgb32(255, 221, 0);
let BLUE = rgb32(0, 77, 168);

// Using an array with more WHITEs makes it the most likely choice for fills.
let COLORS = [RED, YELLOW, BLUE, WHITE, WHITE, WHITE, WHITE, WHITE, WHITE];

// --- Pseudo-Random Number Generator ---
// Plush does not have a built-in RNG, so we use a simple LCG.
let var lcg_seed = 1;
fun rand_init(seed) {
    lcg_seed = seed & 0x7FFFFFFF;
}
// Returns a random integer in the range [0, max_val)
fun rand_int(max_val) {
    // LCG parameters from POSIX C standard
    lcg_seed = (lcg_seed * 1103515245 + 12345) & 0x7FFFFFFF;
    return lcg_seed % max_val;
}

fun max(a, b) {
    if (a > b) return a;
    return b;
}

fun min(a, b) {
    if (a < b) return a;
    return b;
}

// --- Drawing Functions ---

// Draws a filled rectangle in a ByteArray buffer
fun draw_rect(buffer, x, y, w, h, color) {
    // Clamp the rectangle to the buffer boundaries
    let x_start = max(x, 0);
    let y_start = max(y, 0);
    let x_end = min(x + w, WIDTH);
    let y_end = min(y + h, HEIGHT);

    for (let var j = y_start; j < y_end; ++j) {
        for (let var i = x_start; i < x_end; ++i) {
            let idx = j * WIDTH + i;
            buffer.write_u32(idx, color);
        }
    }
}

// --- Recursive Subdivision ---

fun subdivide(buffer, x, y, w, h, depth) {
    // Base Case: Decide whether to stop subdividing.
    // The chance of stopping increases as the rectangle gets smaller.
    let var stop_chance = 15;
    if (w < MIN_SIZE * 2 || h < MIN_SIZE * 2) {
        stop_chance = 85;
    }
    if (depth >= MIN_DEPTH && rand_int(100) < stop_chance) {
        // Fill the rectangle with a random color from the palette.
        let color = COLORS[rand_int(COLORS.len)];
        draw_rect(buffer, x, y, w, h, color);
        return;
    }

    // Recursive Step: Decide on a split direction.
    // Prefer to split along the longer axis.
    if (w > h) {
        // Split vertically
        let range = w - MIN_SIZE * 2;
        if (range <= 0) {
            let color = COLORS[rand_int(COLORS.len)];
            draw_rect(buffer, x, y, w, h, color);
            return;
        }
        let split_offset = rand_int(range) + MIN_SIZE;
        let line_x = x + split_offset;

        // Recurse on the two new sub-rectangles.
        subdivide(buffer, x, y, split_offset, h, depth + 1);
        subdivide(buffer, line_x + LINE_WIDTH, y, w - split_offset - LINE_WIDTH, h, depth + 1);

        // Draw the dividing line.
        draw_rect(buffer, line_x, y, LINE_WIDTH, h, BLACK);
    } else {
        // Split horizontally
        let range = h - MIN_SIZE * 2;
        if (range <= 0) {
            let color = COLORS[rand_int(COLORS.len)];
            draw_rect(buffer, x, y, w, h, color);
            return;
        }
        let split_offset = rand_int(range) + MIN_SIZE;
        let line_y = y + split_offset;

        // Recurse on the two new sub-rectangles.
        subdivide(buffer, x, y, w, split_offset, depth + 1);
        subdivide(buffer, x, line_y + LINE_WIDTH, w, h - split_offset - LINE_WIDTH, depth + 1);

        // Draw the dividing line.
        draw_rect(buffer, x, line_y, w, LINE_WIDTH, BLACK);
    }
}

// --- Main Program ---

fun generate_and_draw(buffer, window) {
    // Re-initialize the random seed to get a new pattern each time.
    rand_init($time_current_ms());

    // Start with a plain white background.
    draw_rect(buffer, 0, 0, WIDTH, HEIGHT, WHITE);

    // Begin the recursive drawing process on the whole canvas.
    subdivide(buffer, 0, 0, WIDTH, HEIGHT, 0);

    // Draw the final image to the window.
    $window_draw_frame(window, buffer);
}

// Set up the graphics buffer and window.
let frame_buffer = ByteArray.with_size(WIDTH * HEIGHT * 4);
let window = $window_create(WIDTH, HEIGHT, "Mondrian Generator (Press Enter for new pattern)", 0);

// Draw the initial pattern.
generate_and_draw(frame_buffer, window);

// Wait for the user to close the window or press Enter.
loop {
    let msg = $actor_recv();
    if (msg instanceof UIEvent) {
        if (msg.kind == 'CLOSE_WINDOW' || (msg.kind == 'KEY_DOWN' && msg.key == 'ESCAPE')) {
            break;
        }
        if (msg.kind == 'KEY_DOWN' && msg.key == 'RETURN') {
            generate_and_draw(frame_buffer, window);
        }
    }
}
