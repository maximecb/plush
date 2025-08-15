fun max(a, b) {
    if (a > b) return a;
    return b;
}

fun min(a, b) {
    if (a < b) return a;
    return b;
}

fun tan(a) {
    let s = a.sin();
    return (1-(s*s).sqrt());
}

class Vector2 {
    init(self, x, y) {
        self.x = x;
        self.y = y;
    }
}

class Vector3 {
    init(self, x, y, z) {
        self.x = x;
        self.y = y;
        self.z = z;
    }
}

// 4x4 Matrix for 3D transformations
class Matrix4 {
    init(self, m) {
        self.m = m; // m is a flat array of 16 elements (row-major)
    }
    // Multiply Vector3 by Matrix4, return Vector3 (no perspective divide)
    mul_vec3(self, v) {
        let var x = self.m[0]*v.x + self.m[1]*v.y + self.m[2]*v.z + self.m[3];
        let var y = self.m[4]*v.x + self.m[5]*v.y + self.m[6]*v.z + self.m[7];
        let var z = self.m[8]*v.x + self.m[9]*v.y + self.m[10]*v.z + self.m[11];
        let w = self.m[12]*v.x + self.m[13]*v.y + self.m[14]*v.z + self.m[15];
        if (w != 0) {
            x = x / w;
            y = y / w;
            z = z / w;
        }
        return Vector3(x, y, z);
    }
}

// Perspective projection matrix (right-handed, fov in radians)
fun perspective_matrix(fov, aspect, near, far) {
    let f = 1.0 / tan((fov / 2.0));
    let nf = 1.0 / (near - far);
    return Matrix4([
        f / aspect, 0, 0, 0,
        0, f, 0, 0,
        0, 0, (far + near) * nf, 2 * far * near * nf,
        0, 0, -1, 0
    ]);
}

// Project Vector3 to Vector2 (NDC [-1,1] to [0,1])
fun project_to_screen(v) {
    // v.x, v.y in [-1,1] -> [0,1]
    return Vector2(0.5 * v.x + 0.5, 0.5 * (1.0 - v.y));
}

// Rasterize a triangle into a ByteArray framebuffer
fun rasterize_triangle(framebuffer, width, height, v0, v1, v2, color) {
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
    if (area == 0) return; // Degenerate triangle

    // Scan through bounding box
    for (let var y = minY; y <= maxY; y = y + 1) {
        for (let var x = minX; x <= maxX; x = x + 1) {
            // Compute barycentric coordinates
            let w0 = ((y1 - y2) * (x - x2) + (x2 - x1) * (y - y2));
            let w1 = ((y2 - y0) * (x - x2) + (x0 - x2) * (y - y2));
            let w2 = area - w0 - w1;

            // Check if point is inside triangle
            if ((w0 >= 0 && w1 >= 0 && w2 >= 0) || (w0 <= 0 && w1 <= 0 && w2 <= 0)) {
                // Fill with provided face color
                let index = y * width + x;
                framebuffer.write_u32(index, color);
            }
        }
    }
}

let width = 600;
let height = 600;
let framebuffer = ByteArray.with_size(width * height * 4);

// Clear to black
framebuffer.fill_u32(0, width * height, 0xFF000000);


// 3D triangle vertices (in world space)
// Define 8 vertices of a cube centered at (0,0,2) with size 1
let cube_vertices = [
    Vector3(-0.5, -0.5, 1.5), // 0: left-bottom-near
    Vector3(0.5, -0.5, 1.5),  // 1: right-bottom-near
    Vector3(0.5, 0.5, 1.5),   // 2: right-top-near
    Vector3(-0.5, 0.5, 1.5),  // 3: left-top-near
    Vector3(-0.5, -0.5, 2.5), // 4: left-bottom-far
    Vector3(0.5, -0.5, 2.5),  // 5: right-bottom-far
    Vector3(0.5, 0.5, 2.5),   // 6: right-top-far
    Vector3(-0.5, 0.5, 2.5)   // 7: left-top-far
];

// Cube faces as triangles (each face = 2 triangles)
let cube_faces = [
    // Near face
    [0, 1, 2], [0, 2, 3],
    // Far face
    [4, 6, 5], [4, 7, 6],
    // Left face
    [0, 3, 7], [0, 7, 4],
    // Right face
    [1, 5, 6], [1, 6, 2],
    // Top face
    [3, 2, 6], [3, 6, 7],
    // Bottom face
    [0, 4, 5], [0, 5, 1]
];

// Perspective projection parameters
let fov = 1.2; // radians (~69 deg)
let aspect = width / height;
let near = 1.0;
let far = 10.0;
let proj = perspective_matrix(fov, aspect, near, far);

// Project all cube vertices
let proj_vertices = [];
for (let var i = 0; i < 8; i = i + 1) {
    proj_vertices.push(project_to_screen(proj.mul_vec3(cube_vertices[i])));
}

// Create a window
let window = $window_create(width, height, "3D Cube Rasterizer", 0);

// Rasterize all cube triangles

// Define a color for each face (ARGB)
let face_colors = [
    0xFFFF0000, // Near face - red
    0xFFFF0000, // Near face - red
    0xFF00FF00, // Far face - green
    0xFF00FF00, // Far face - green
    0xFF0000FF, // Left face - blue
    0xFF0000FF, // Left face - blue
    0xFFFFFF00, // Right face - yellow
    0xFFFFFF00, // Right face - yellow
    0xFFFF00FF, // Top face - magenta
    0xFFFF00FF, // Top face - magenta
    0xFF00FFFF, // Bottom face - cyan
    0xFF00FFFF  // Bottom face - cyan
];

let var start_time = $time_current_ms();
for (let var i = 0; i < cube_faces.len; i = i + 1) {
    let f = cube_faces[i];
    let color = face_colors[i];
    rasterize_triangle(framebuffer, width, height,
        proj_vertices[f[0]], proj_vertices[f[1]], proj_vertices[f[2]], color);
}
let var end_time = $time_current_ms() - start_time;
$println("Cube rasterization time: " + end_time.to_s() + "ms");

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
