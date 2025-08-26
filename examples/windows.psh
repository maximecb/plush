// Draggable, movable window panels example

let WINDOW_WIDTH = 1024;
let WINDOW_HEIGHT = 768;

// 32-bit color constants (AARRGGBB format)
let WIN_BLUE = 0xFF3A6EA5;
let PANEL_GRAY = 0xFFC0C0C0;
let TITLE_BLUE = 0xFF000080;
let WHITE = 0xFFFFFFFF;
let BLACK = 0xFF000000;
let RED = 0xFFEE0000;
let DARK_GRAY = 0xFF808080;

// Helper functions
fun max(a, b) {
    if (a > b) { return a; }
    return b;
}

fun min(a, b) {
    if (a < b) { return a; }
    return b;
}

// Helper function to draw a rectangle using fill_u32 for efficiency
fun draw_rect(frame_buffer, x, y, width, height, color) {
    let x_start = max(0, x);
    let y_start = max(0, y);
    let x_end = min(WINDOW_WIDTH, x + width);
    let y_end = min(WINDOW_HEIGHT, y + height);

    let clipped_width = x_end - x_start;

    if (clipped_width <= 0) {
        return;
    }

    for (let var j = y_start; j < y_end; ++j) {
        let start_index = j * WINDOW_WIDTH + x_start;
        frame_buffer.fill_u32(start_index, clipped_width, color);
    }
}

// Helper function to draw an X
fun draw_x(frame_buffer, x, y, size, color) {
    for (let var i = 0; i < size; ++i) {
        // Top-left to bottom-right diagonal
        let px1 = x + i;
        let py1 = y + i;
        if (px1 >= 0 && px1 < WINDOW_WIDTH && py1 >= 0 && py1 < WINDOW_HEIGHT) {
            frame_buffer.write_u32(py1 * WINDOW_WIDTH + px1, color);
        }

        // Top-right to bottom-left diagonal
        let px2 = x + size - 1 - i;
        let py2 = y + i;
        if (px2 >= 0 && px2 < WINDOW_WIDTH && py2 >= 0 && py2 < WINDOW_HEIGHT) {
            frame_buffer.write_u32(py2 * WINDOW_WIDTH + px2, color);
        }
    }
}

class Panel {
    init(self, x, y, width, height, title) {
        self.x = x;
        self.y = y;
        self.width = width;
        self.height = height;
        self.title = title;
        self.is_open = true;
        self.dragging = false;
        self.drag_offset_x = 0;
        self.drag_offset_y = 0;
    }

    // Draw the panel into a frame buffer
    draw(self, frame_buffer) {
        if (!self.is_open) {
            return;
        }

        let title_bar_height = 20;
        let close_button_size = 14;
        let border = 3;

        // Draw embossed border
        draw_rect(frame_buffer, self.x, self.y, self.width, 1, WHITE);
        draw_rect(frame_buffer, self.x, self.y, 1, self.height, WHITE);
        draw_rect(frame_buffer, self.x + self.width - 1, self.y, 1, self.height, DARK_GRAY);
        draw_rect(frame_buffer, self.x, self.y + self.height - 1, self.width, 1, DARK_GRAY);

        // Draw the main panel background
        draw_rect(frame_buffer, self.x + 1, self.y + 1, self.width - 2, self.height - 2, PANEL_GRAY);

        // Draw the title bar
        draw_rect(frame_buffer, self.x + 1, self.y + 1, self.width - 2, title_bar_height, TITLE_BLUE);

        // Draw the close button
        let close_button_x = self.x + self.width - close_button_size - border - 1;
        let close_button_y = self.y + border;
        draw_rect(frame_buffer, close_button_x, close_button_y, close_button_size, close_button_size, RED);

        // Draw the X in the close button
        draw_x(frame_buffer, close_button_x + 3, close_button_y + 3, close_button_size - 6, WHITE);
    }

    // Handle mouse press events
    on_mouse_down(self, x, y) {
        if (!self.is_open) {
            return false;
        }

        let title_bar_height = 20;
        let close_button_size = 14;
        let border = 3;
        let close_button_x = self.x + self.width - close_button_size - border - 1;
        let close_button_y = self.y + border;

        if (x >= close_button_x && x < close_button_x + close_button_size &&
            y >= close_button_y && y < close_button_y + close_button_size) {
            self.is_open = false;
            return true;
        }

        if (x >= self.x && x < self.x + self.width &&
            y >= self.y && y < self.y + title_bar_height) {
            self.dragging = true;
            self.drag_offset_x = x - self.x;
            self.drag_offset_y = y - self.y;
            return true;
        }

        return false;
    }

    on_mouse_move(self, x, y) {
        if (self.dragging) {
            self.x = x - self.drag_offset_x;
            self.y = y - self.drag_offset_y;
            return true;
        }
        return false;
    }

    on_mouse_up(self) {
        if (self.dragging) {
            self.dragging = false;
            return true;
        }
        return false;
    }
}

fun redraw(window_id, frame_buffer, panels) {
    let start_time = $time_current_ms();

    // Clear the buffer
    frame_buffer.fill_u32(0, WINDOW_WIDTH * WINDOW_HEIGHT, WIN_BLUE);

    // Draw all panels
    for (let var i = 0; i < panels.len; ++i) {
        panels[i].draw(frame_buffer);
    }

    let end_time = $time_current_ms();
    $print("redraw time: ");
    $print(end_time - start_time);
    $print(" ms\n");

    // Update the screen
    $window_draw_frame(window_id, frame_buffer);
}

fun main() {
    let window_id = $window_create(WINDOW_WIDTH, WINDOW_HEIGHT, "Draggable Panels", 0);
    let frame_buffer = ByteArray.with_size(WINDOW_WIDTH * WINDOW_HEIGHT * 4);
    let var panels = [];

    panels.push(Panel(50, 50, 300, 200, "Panel 1"));
    panels.push(Panel(400, 100, 350, 250, "Panel 2"));
    panels.push(Panel(100, 400, 280, 180, "Panel 3"));
    panels.push(Panel(550, 450, 300, 200, "Panel 4"));

    // Initial draw
    redraw(window_id, frame_buffer, panels);

    while (true) {
        // Block and wait for an event
        let event = $actor_recv();

        if (event.kind == "CLOSE_WINDOW" || (event.kind == "KEY_DOWN" && event.key == "ESCAPE")) {
            break;
        }

        if (event.kind == "MOUSE_DOWN") {
            for (let var i = panels.len - 1; i >= 0; --i) {
                if (panels[i].on_mouse_down(event.x, event.y)) {
                    // Bring clicked panel to front
                    let p = panels[i];

                    // Rebuild the array to move p to the end
                    let var new_panels = [];
                    for (let var j = 0; j < panels.len; ++j) {
                        if (i != j) {
                            new_panels.push(panels[j]);
                        }
                    }
                    new_panels.push(p);
                    panels = new_panels;

                    break;
                }
            }
        }

        if (event.kind == "MOUSE_MOVE") {
            for (let var i = 0; i < panels.len; ++i) {
                panels[i].on_mouse_move(event.x, event.y);
            }
        }

        if (event.kind == "MOUSE_UP") {
            for (let var i = 0; i < panels.len; ++i) {
                panels[i].on_mouse_up();
            }
        }

        redraw(window_id, frame_buffer, panels);
    }
}

main();