let TILE_SIZE = 75; // Size of each tile for parallel rendering

fun min(a, b) {
    if (a < b) return a;
    return b;
}

fun max(a, b) {
    if (a > b) return a;
    return b;
}

fun tan(a) {
    return(a.sin()/a.cos());
}

// Image class for framebuffer management
class Image {
    init(self, width, height) {
        self.width = width;
        self.height = height;
        self.bytes = ByteArray.with_size(4 * width * height);
    }

    // The color is specified as an u32 value in RGBA32 format
    set_pixel(self, x, y, color) {
        let idx = y * self.width + x;
        self.bytes.write_u32(idx, color);
    }

    // Fill with a color
    fill(self, color) {
        self.bytes.fill_u32(0, self.width * self.height, color);
    }

    // Copy a source image into this image at a given position
    blit(self, src_img, dst_x, dst_y) {
        let var dst_x = dst_x;
        let var dst_y = dst_y;
        let var src_x = 0;
        let var src_y = 0;
        let var width = src_img.width;
        let var height = src_img.height;

        if (dst_x < 0) {
            src_x = -dst_x;
            width = width + dst_x;
            dst_x = 0;
        }

        if (dst_y < 0) {
            src_y = -dst_y;
            height = height + dst_y;
            dst_y = 0;
        }

        if (dst_x + width > self.width) {
            width = self.width - dst_x;
        }

        if (dst_y + height > self.height) {
            height = self.height - dst_y;
        }

        if (width <= 0 || height <= 0) {
            return;
        }

        // Number of bytes per row of the images
        let dst_pitch = self.width * 4;
        let src_pitch = src_img.width * 4;

        for (let var j = 0; j < height; ++j) {
            let src_idx = (src_y + j) * src_pitch + src_x * 4;
            let dst_idx = (dst_y + j) * dst_pitch + dst_x * 4;
            self.bytes.memcpy(dst_idx, src_img.bytes, src_idx, width * 4);
        }
    }
}

class Vec3 {
    init(self, x, y, z) {
        self.x = x;
        self.y = y;
        self.z = z;
    }

    // Vector addition
    add(self, other) {
        return Vec3(self.x + other.x, self.y + other.y, self.z + other.z);
    }

    // Vector subtraction
    sub(self, other) {
        return Vec3(self.x - other.x, self.y - other.y, self.z - other.z);
    }

    // Scalar multiplication
    mul(self, scalar) {
        return Vec3(self.x * scalar, self.y * scalar, self.z * scalar);
    }

    // Dot product
    dot(self, other) {
        return self.x * other.x + self.y * other.y + self.z * other.z;
    }

    // Cross product
    cross(self, other) {
        return Vec3(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x
        );
    }

    // Length squared
    length_squared(self) {
        return self.dot(self);
    }

    // Normalize vector
    normalize(self) {
        let len = self.length_squared().sqrt();
        if (len == 0.0) {
            return Vec3(0.0, 0.0, 0.0);
        }
        return Vec3(self.x / len, self.y / len, self.z / len);
    }
}

// Screen setup
let WIDTH = 600;
let HEIGHT = 600;

// Queue for UI events that arrive while renderer is waiting for TileResult
let pending_ui_events = [];
let var should_exit = false;

// Vertices setup
let a = Vec3(-1, +1, -1);
let b = Vec3(-1, -1, -1);
let c = Vec3(+1, -1, -1);
let d = Vec3(+1, +1, -1);

let e = Vec3(-1, -1, 1);
let f = Vec3(+1, -1, 1);
let g = Vec3(+1, +1, 1);
let h = Vec3(-1, +1, 1);

let triangles = [
    // front
    [a, c, b],
    [a, d, c],
    // right
    [d, f, c],
    [d, g, f],
    // back
    [g, h, e],
    [g, e, f],
    // left
    [h, a, b],
    [h, b, e],
    // top
    [b, f, e],
    [b, c, f],
    // bottom
    [a, h, g],
    [a, g, d]
];

// Array of vertices (for index)
let vertices = [a, b, c, d, e, f, g, h];

let trianglesIndex = [
    // front
    [0, 2, 1],
    [0, 3, 2],
    // right
    [3, 5, 2],
    [3, 6, 5],
    // back
    [6, 7, 4],
    [6, 4, 5],
    // left
    [7, 0, 1],
    [7, 1, 4],
    // top
    [1, 5, 4],
    [1, 2, 5],
    // bottom
    [0, 7, 6],
    [0, 6, 3]
];

// Projection matrix setup
let fNear = -0.1;
let fFar = 1000.0;
let fFov = 1.5707963; // radians
let fAspectRatio = WIDTH/HEIGHT.to_f(); // float
let fFovRad = 1/tan(fFov*0.5);

let palette = [
    // front
    0xFFFFD166,
    0xFFFFD166,
    // right
    0xFF6B5B95,
    0xFF6B5B95,
    // back
    0xFF88B04B,
    0xFF88B04B,
    // left
    0xFFF7CAC9,
    0xFFF7CAC9,
    // top
    0xFF92A8D1,
    0xFF92A8D1,
    // bottom
    0xFF955251,
    0xFF955251,
];

fun initMat4(m) {
    for (let var i = 0; i < 4; ++i) {
        let row = Array.with_size(4, 0.0);
        m.push(row);
    }
}

// Multiply a Vec3 by a 4x4 matrix
fun multMatVec(i, m) {
    let o = Vec3(0, 0, 0);

    o.x = i.x * m[0][0] + i.y * m[1][0] + i.z * m[2][0] + m[3][0];
    o.y = i.x * m[0][1] + i.y * m[1][1] + i.z * m[2][1] + m[3][1];
    o.z = i.x * m[0][2] + i.y * m[1][2] + i.z * m[2][2] + m[3][2];
    let w = i.x * m[0][3] + i.y * m[1][3] + i.z * m[2][3] + m[3][3];

    if (w.floor() != 0) {
        o.x = o.x/w;
        o.y = o.y/w;
    }

    return o;
}

// Rasterize a triangle into a tile
fun rasterize_triangle_tile(v0, v1, v2, color, tile_img, tile_x, tile_y, tile_w, tile_h) {
    // Convert vertices to integer coordinates
    let x0 = v0.x.floor();
    let y0 = v0.y.floor();
    let x1 = v1.x.floor();
    let y1 = v1.y.floor();
    let x2 = v2.x.floor();
    let y2 = v2.y.floor();

    // Compute bounding box, clipped to tile
    let minX = max(tile_x, min(x0, min(x1, x2)));
    let maxX = min(tile_x + tile_w - 1, max(x0, max(x1, x2)));
    let minY = max(tile_y, min(y0, min(y1, y2)));
    let maxY = min(tile_y + tile_h - 1, max(y0, max(y1, y2)));

    // Precompute barycentric coordinate divisors
    let area = (y1 - y2) * (x0 - x2) + (x2 - x1) * (y0 - y2);
    if (area == 0) return; // Degenerate triangle

    // Scan through bounding box
    for (let var y = minY; y <= maxY; y = y + 1) {
        for (let var x = minX; x <= maxX; x = x + 1) {
            // Compute barycentric coordinates
            let w0 = (y1 - y2) * (x - x2) + (x2 - x1) * (y - y2);
            let w1 = (y2 - y0) * (x - x2) + (x0 - x2) * (y - y2);
            let w2 = area - w0 - w1;

            // Check if point is inside triangle
            if (w0 <= 0 && w1 <= 0 && w2 <= 0) {
                // Convert to tile-local coordinates
                let local_x = x - tile_x;
                let local_y = y - tile_y;
                if (local_x >= 0 && local_x < tile_w && local_y >= 0 && local_y < tile_h) {
                    tile_img.set_pixel(local_x, local_y, color);
                }
            }
        }
    }
}

// Cube rendering data structure
class CubeRenderData {
    init(self, fTheta) {
        self.fTheta = fTheta;

        // Create matrices
        self.matRotX = [];
        self.matRotZ = [];
        self.matProj = [];

        initMat4(self.matRotX);
        initMat4(self.matRotZ);
        initMat4(self.matProj);

        // Compute rotation matrices
        self.matRotX[0][0] = 1.0;
        self.matRotX[1][1] = (fTheta * 0.5).cos();
        self.matRotX[1][2] = (fTheta * 0.5).sin();
        self.matRotX[2][1] = -((fTheta * 0.5).sin());
        self.matRotX[2][2] = (fTheta * 0.5).cos();
        self.matRotX[3][3] = 1.0;

        self.matRotZ[0][0] = fTheta.cos();
        self.matRotZ[0][1] = (fTheta).sin();
        self.matRotZ[1][0] = -((fTheta).sin());
        self.matRotZ[1][1] = fTheta.cos();
        self.matRotZ[2][2] = 1.0;
        self.matRotZ[3][3] = 1.0;

        // Projection
        self.matProj[0][0] = fAspectRatio*fFovRad;
        self.matProj[1][1] = fFovRad;
        self.matProj[2][2] = fFar / (fFar - fNear);
        self.matProj[2][3] = 1.0;
        self.matProj[3][2] = (-fFar * fNear) / (fFar - fNear);

        // Compute transformed vertices
        self.rota = [];
        for (let var i = 0; i < 8; ++i) {
            self.rota.push(multMatVec(vertices[i], self.matRotZ));
        }
        for (let var i = 0; i < 8; ++i) {
            self.rota[i] = multMatVec(self.rota[i], self.matRotX);
        }

        // Compute normals
        self.normal = [];
        for (let var i = 0; i < triangles.len; ++i) {
            let idxs = trianglesIndex[i];
            let v0 = self.rota[idxs[0]];
            let v1 = self.rota[idxs[1]];
            let v2 = self.rota[idxs[2]];

            let line1 = v1.sub(v0);
            let line2 = v2.sub(v0);
            let var normal = line1.cross(line2);
            normal = normal.normalize();

            self.normal.push(normal);
        }

        // Project vertices
        self.proj = [];
        for (let var i = 0; i < 8; ++i) {
            self.proj.push(multMatVec(Vec3(self.rota[i].x, self.rota[i].y, self.rota[i].z + 3), self.matProj));
        }
        for (let var i = 0; i < 8; ++i) {
            self.proj[i] = Vec3(
                (self.proj[i].x + 1) * 0.5 * WIDTH,
                (self.proj[i].y + 1) * 0.5 * HEIGHT,
                self.proj[i].z
            );
        }

        self.camera = Vec3(0, 0, -3);
    }
}

// Convert ARGB values in the range [0, 255] to a u32 encoding
fun to_u32(a, r, g, b) {
    return (a << 24) | (r << 16) | (g << 8) | b;
}

// Extract ARGB components from a u32 color value
fun to_argb(color) {
    let a = (color >> 24) & 0xFF;
    let r = (color >> 16) & 0xFF;
    let g = (color >> 8) & 0xFF;
    let b = color & 0xFF;
    return [a, r, g, b];
}

// Light direction (normalized vector pointing toward the light source)
let light_direction = Vec3(-0.5,  -0.7,  -2.0).normalize();
let ambient_min = 0.008; // Minimum light level

// Apply directional lighting to a color based on surface normal
fun apply_lighting(base_color, normal) {
    // Calculate lighting intensity (dot product of normal and light direction)
    let light_intensity = max(ambient_min, normal.dot(light_direction));

    let argb = to_argb(base_color);
    let a = argb[0];
    let r = argb[1];
    let g = argb[2];
    let b = argb[3];

    // Apply lighting to RGB components
    let lit_r = min(255, (r.to_f() * light_intensity).floor());
    let lit_g = min(255, (g.to_f() * light_intensity).floor());
    let lit_b = min(255, (b.to_f() * light_intensity).floor());

    // Reconstruct u32 color
    return to_u32(a, lit_r, lit_g, lit_b);
}

// Render a tile of the cube
fun render_cube_tile(cube_data, tile_x, tile_y, tile_w, tile_h) {
    let tile_img = Image(tile_w, tile_h);
    tile_img.fill(0xFF000000);

    // Draw each triangle
    for (let var i = 0; i < triangles.len; ++i) {
        let triangle = triangles[i];
        let var points = [];
        let var pointsRota = [];
        for (let var j = 0; j < 3; ++j) {
            for (let var k = 0; k < vertices.len; ++k) {
                if (vertices[k] == triangle[j]) {
                    points.push(cube_data.proj[k]);
                    pointsRota.push(cube_data.rota[k]);
                }
            }
        }
        // Backface culling
        if (cube_data.normal[i].dot(pointsRota[0].sub(cube_data.camera)) < 0) {
            // Apply lighting to the base color
            let base_color = palette[i];
            let lit_color = apply_lighting(base_color, cube_data.normal[i]);
            rasterize_triangle_tile(points[0], points[1], points[2], lit_color, tile_img, tile_x, tile_y, tile_w, tile_h);
        }
    }

    return tile_img;
}

// Tile render request
class TileRequest {
    init(self, cube_data, tile_x, tile_y, tile_w, tile_h) {
        self.cube_data = cube_data;
        self.tile_x = tile_x;
        self.tile_y = tile_y;
        self.tile_w = tile_w;
        self.tile_h = tile_h;
    }
}

// Tile render result
class TileResult {
    init(self, tile_img, tile_x, tile_y, actor_id) {
        self.tile_img = tile_img;
        self.tile_x = tile_x;
        self.tile_y = tile_y;
        self.actor_id = actor_id;
    }
}

// Actor loop for rendering tiles
fun actor_loop() {
    while (true) {
        let msg = $actor_recv();

        // Done rendering
        if (msg == nil)
            return;

        let tile_img = render_cube_tile(
            msg.cube_data,
            msg.tile_x,
            msg.tile_y,
            msg.tile_w,
            msg.tile_h
        );

        let result = TileResult(tile_img, msg.tile_x, msg.tile_y, $actor_id());
        $actor_send($actor_parent(), result);
    }
}

// Parallel cube rendering
fun render_cube_parallel(fTheta) {
    let num_actors = 16;

    // Create the actors
    let actor_ids = [];
    for (let var i = 0; i < num_actors; ++i)
        actor_ids.push($actor_spawn(actor_loop));

    // Pre-compute cube data
    let cube_data = CubeRenderData(fTheta);

    // Create a list of tile requests
    let requests = [];
    for (let var y = 0; y < HEIGHT; y = y + TILE_SIZE) {
        for (let var x = 0; x < WIDTH; x = x + TILE_SIZE) {
            let tile_w = min(TILE_SIZE, WIDTH - x);
            let tile_h = min(TILE_SIZE, HEIGHT - y);
            requests.push(TileRequest(cube_data, x, y, tile_w, tile_h));
        }
    }
    let num_tiles = requests.len;

    // Image to render into
    let image = Image(WIDTH, HEIGHT);

    let start_time = $time_current_ms();

    // Send initial requests to each actor
    for (let var i = 0; i < num_actors && requests.len > 0; ++i) {
        $actor_send(actor_ids[i], requests.pop());
    }

    // Receive all the render results. Buffer UI events that arrive here.
    let var num_received = 0;
    while (num_received < num_tiles) {
        let msg = $actor_recv();

        // If a UI event arrives while we're waiting for tiles, buffer it for the
        // main loop to process and continue waiting for tile results.
        if (msg instanceof UIEvent) {
            pending_ui_events.push(msg);
            continue;
        }

        // Ignore spurious nils
        if (msg == nil) {
            continue;
        }

        // Send more work to this actor if available
        if (requests.len > 0) {
            $actor_send(msg.actor_id, requests.pop());
        }

        image.blit(msg.tile_img, msg.tile_x, msg.tile_y);
        ++num_received;
    }

    let render_time = $time_current_ms() - start_time;
    let fps = (1000 / render_time).floor();
    $println("Parallel render time: " + render_time.to_s() + "ms (" + fps.to_s() + " fps)");

    // Tell actors to terminate
    for (let var i = 0; i < num_actors; ++i) {
        $actor_send(actor_ids[i], nil);
    }

    return image;
}

// Single-threaded rendering for comparison
fun render_cube_single(fTheta) {
    let start_time = $time_current_ms();
    let cube_data = CubeRenderData(fTheta);
    let image = render_cube_tile(cube_data, 0, 0, WIDTH, HEIGHT);
    let render_time = $time_current_ms() - start_time;
    // $println("Single thread render time: " + render_time.to_s() + "ms");
    return image;
}

// Main rendering loop
let window = $window_create(WIDTH, HEIGHT, "Cube (Parallel)", 0);
let var fTheta = 0.0;
let var previous_time = $time_current_ms();

// Switch between parallel and single-threaded rendering
let var use_parallel = true;

loop {
    let current_time = $time_current_ms();
    let delta_time = (current_time - previous_time).to_f() * 0.001;
    fTheta = fTheta + delta_time * 0.85;
    previous_time = current_time;

    if (should_exit) {
        break;
    }

    let var image = nil;
    if (use_parallel) {
        image = render_cube_parallel(fTheta);
    } else {
        image = render_cube_single(fTheta);
    }

    $window_draw_frame(window, image.bytes);

    // Collect any UI events that may have arrived during rendering
    while (true) {
        let ev = $actor_poll();
        if (ev == nil) break;
        pending_ui_events.push(ev);
    }

    // Process pending UI events (those collected while rendering)
    while (pending_ui_events.len > 0) {
        let ev = pending_ui_events.pop();
        if (!(ev instanceof UIEvent)) continue;
        if (ev.kind == 'CLOSE_WINDOW' || (ev.kind == 'KEY_DOWN' && ev.key == 'ESCAPE')) {
            should_exit = true;
            break;
        }
        if (ev.kind == 'KEY_DOWN' && ev.key == 'SPACE') {
            use_parallel = !use_parallel;
            $println("Switched to " + (use_parallel ? "parallel" : "single-threaded") + " rendering");
        }
    }

    if (should_exit) break;

    $actor_sleep(16);
}
