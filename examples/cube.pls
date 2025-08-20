fun min(a, b) {
    if (a < b) return a;
    return b;
}

fun max(a, b) {
    if (a > b) return a;
    return b;
}

// tan(a) = sin(a)/sin((pi/2)-a)
fun tan(a) {
    return(a.sin()/(1.5707963-a).sin());
}

// cos using sin identity
fun cos(a) {
    return((a + 1.5707963).sin());
}

// Empty 4x4 matrix
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

// Calculate X rotation matrix
fun compute_matX(matRotX, fTheta) {
    // Rotation X
    matRotX[0][0] = 1.0;
    matRotX[1][1] = cos(fTheta * 0.5);
    matRotX[1][2] = (fTheta * 0.5).sin();
    matRotX[2][1] = -((fTheta * 0.5).sin());
    matRotX[2][2] = cos(fTheta * 0.5);
    matRotX[3][3] = 1.0;

    return matRotX;
}

// Calculate Z rotation matrix
fun compute_matZ(matRotZ, fTheta) {
    // Rotation Z
    matRotZ[0][0] = cos(fTheta);
    matRotZ[0][1] = (fTheta).sin();
    matRotZ[1][0] = -((fTheta).sin());
    matRotZ[1][1] = cos(fTheta);
    matRotZ[2][2] = 1.0;
    matRotZ[3][3] = 1.0;
    return matRotZ;
}

// Draw a line between v0(x0, y0) and v1(x1, y1) using Bresenham's algorithm
fun draw_line(v0, v1, width, height, framebuffer) {
    let ix0 = v0.x.floor();
    let iy0 = v0.y.floor();
    let ix1 = v1.x.floor();
    let iy1 = v1.y.floor();

    let dx = (ix1 - ix0).abs();
    let dy = -(iy1 - iy0).abs();
    let sx = ix0 < ix1 ? 1 : -1;
    let sy = iy0 < iy1 ? 1 : -1;
    let var err = dx + dy;

    let var x = ix0;
    let var y = iy0;
    while (true) {
        if (x >= 0 && x < width && y >= 0 && y < height) {
            let idx = (y*width + x);
            framebuffer.write_u32(idx, 0xFFFFFFFF);
        }
        if (x == ix1 && y == iy1) break;
        let var e2 = 2 * err;
        if (e2 >= dy) {
            err = err + dy;
            x = x + sx;
        }
        if (e2 <= dx) {
            err = err + dx;
            y = y + sy;
        }
    }
}

// Draw a triangle by drawing its three edges
fun draw_triangle(v0, v1, v2, width, height, framebuffer) {
    draw_line(v0, v1, width, height, framebuffer);
    draw_line(v1, v2, width, height, framebuffer);
    draw_line(v2, v0, width, height, framebuffer);
}

// Rasterize a triangle into a ByteArray framebuffer
fun rasterize_triangle(v0, v1, v2, palette, paletteIndex, width, height, framebuffer) {
    // Convert vertices to integer coordinates
    let x0 = v0.x.floor();
    let y0 = v0.y.floor();
    let x1 = v1.x.floor();
    let y1 = v1.y.floor();
    let x2 = v2.x.floor();
    let y2 = v2.y.floor();

    // Compute bounding box
    let minX = max(0, min(x0, min(x1, x2)));
    let maxX = min(width - 1, max(x0, max(x1, x2)));
    let minY = max(0, min(y0, min(y1, y2)));
    let maxY = min(height - 1, max(y0, max(y1, y2)));

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
                let index = y * width + x;
                // Select color using provided index
                let color = palette[paletteIndex % palette.len];
                framebuffer.write_u32(index, color);
            }
        }
    }
}

// Rotate vertices
fun rota_vertices(vertices, matRotX, matRotZ) {
    let var rota = [];

    for (let var i = 0; i < 8; ++i) {
        rota.push(multMatVec(vertices[i], matRotZ));
    }

    for (let var i = 0; i < 8; ++i) {
        rota[i] = multMatVec(rota[i], matRotX);
    }
    return rota;
}

// Calculate normal based on rotated vertices
fun calc_normal(rota, triangles, trianglesIndex) {
    let n = [];
    for (let var i = 0; i < triangles.len; ++i) {
        let idxs = trianglesIndex[i];
        let v0 = rota[idxs[0]];
        let v1 = rota[idxs[1]];
        let v2 = rota[idxs[2]];

        let line1 = v1.sub(v0);
        let line2 = v2.sub(v0);
        let var normal = line1.cross(line2);
        normal = normal.normalize();

        //$print("x: " + normal.x.to_s() + " ");
        //$print("y: " + normal.y.to_s() + " ");
        //$print("z: " + normal.z.to_s() + " ");
        //$println("");

        n.push(normal);
    }
    return n;
}

// Calculate projected vertices
fun proj_vertices(rota, matProj, width, height) {
    let var proj = [];

    for (let var i = 0; i < 8; ++i) {
        proj.push(multMatVec(Vec3(rota[i].x, rota[i].y, rota[i].z + 3), matProj));
    }

    for (let var i = 0; i < 8; ++i) {
        proj[i] = Vec3(
            (proj[i].x + 1) * 0.5 * width,
            (proj[i].y + 1) * 0.5 * height,
            proj[i].z
        );
    }
    return proj;
}

fun draw_cube(rota, proj, normal, vertices, triangles, camera, width, height, framebuffer, palette) {
    // Draw each triangle
    for (let var i = 0; i < triangles.len; ++i) {
        let triangle = triangles[i];
        let var points = [];
        let var pointsRota = [];
        for (let var j = 0; j < 3; ++j) {
            for (let var k = 0; k < vertices.len; ++k) {
                if (vertices[k] == triangle[j]) {
                    points.push(proj[k]);
                    pointsRota.push(rota[k]);
                }
            }
        }
        if (normal[i].dot(pointsRota[0].sub(camera)) < 0) {
            //draw_triangle(points[0], points[1], points[2], width, height, framebuffer);
            rasterize_triangle(points[0], points[1], points[2], palette, i, width, height, framebuffer);
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

// Vertices setup
// +X points right,
// +Y points down,
// +Z points into the screen.

let a = Vec3(-1, +1, -1);
let b = Vec3(-1, -1, -1);
let c = Vec3(+1, -1, -1);
let d = Vec3(+1, +1, -1);

let e = Vec3(-1, -1, 1);
let f = Vec3(+1, -1, 1);
let g = Vec3(+1, +1, 1);
let h = Vec3(-1, +1, 1);

let triangles = [
    //front
    [a, c, b],
    [a, d, c],
    //right
    [d, f, c],
    [d, g, f],
    //back
    [g, h, e],
    [g, e, f],
    //left
    [h, a, b],
    [h, b, e],
    //top
    [b, f, e],
    [b, c, f],
    //bottom
    [a, h, g],
    [a, g, d]
];

// Array of vertices (for indices)
let vertices = [a, b, c, d, e, f, g, h];

// Hard-coded indices
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

// Projecton matrix setup
let fNear = 0.1;
let fFar = 1000.0;
let fFov = 1.5707963; // radians
let fAspectRatio = WIDTH/HEIGHT.to_f(); // float
let fFovRad = 1/tan(fFov*0.5);

// Time setup
let var fTheta = $time_current_ms().to_f();

let var matIdent = [];
let var matRotX = [];
let var matRotZ = [];
let var matProj = [];

initMat4(matIdent);
initMat4(matRotX);
initMat4(matRotZ);
initMat4(matProj);

// matIdent
matIdent[0][0] = 1.0;
matIdent[1][1] = 1.0;
matIdent[2][2] = 1.0;
matIdent[3][3] = 1.0;

// Projection
matProj[0][0] = fAspectRatio*fFovRad;
matProj[1][1] = fFovRad;
matProj[2][2] = fFar / (fFar - fNear);
matProj[2][3] = 1.0;
matProj[3][2] = (-fFar * fNear) / (fFar - fNear);

// Matrix print (for testing)
//for (let var i = 0; i < 4; ++i) {
//    for (let var j = 0; j < 4; ++j) {
//        $print(matIdent[i][j].to_s() + " ");
//    }
//    $println("");
//}

// Create framebuffer
let framebuffer = ByteArray.with_size(WIDTH * HEIGHT * 4);
framebuffer.fill_u32(0, WIDTH * HEIGHT, 0xFF000000);

let palette = [
    // front
    0xFFDC3C3C,
    0xFFDC3C3C,
    // right
    0xFF3CBD5E,
    0xFF3CBD5E,
    // back
    0xFF3C6BDC,
    0xFF3C6BDC,
    // left
    0xFF3CBDBD,
    0xFF3CBDBD,
    // top
    0xFFE6C93C,
    0xFFE6C93C,
    // bottom
    0xFFBD3CBD,
    0xFFBD3CBD,
];

let camera = Vec3(0, 0, -3);

matRotX = compute_matX(matRotX, fTheta);
matRotZ = compute_matZ(matRotZ,fTheta);
let rota = rota_vertices(vertices, matRotX, matRotZ);
let proj = proj_vertices(rota, matProj, WIDTH, HEIGHT);
let normal = calc_normal(rota, triangles, trianglesIndex);
draw_cube(rota, proj, normal, vertices, triangles, camera, WIDTH, HEIGHT, framebuffer, palette);

// Draw in window
let window = $window_create(WIDTH, HEIGHT, "Cube", 0);
$window_draw_frame(window, framebuffer);

loop {
    let msg = $actor_poll();

    framebuffer.fill_u32(0, WIDTH * HEIGHT, 0xFF000000);
    fTheta = $time_current_ms().to_f() * 0.001;

    matRotX = compute_matX(matRotX, fTheta);
    matRotZ = compute_matZ(matRotZ,fTheta);
    let rota = rota_vertices(vertices, matRotX, matRotZ);
    let proj = proj_vertices(rota, matProj, WIDTH, HEIGHT);
    let normal = calc_normal(rota, triangles, trianglesIndex);
    draw_cube(rota, proj, normal, vertices, triangles, camera, WIDTH, HEIGHT, framebuffer, palette);

    $window_draw_frame(window, framebuffer);

    if (msg == nil) {
        continue;
    }
    if (!(msg instanceof UIEvent)) {
        continue;
    }
    if (msg.kind == 'CLOSE_WINDOW' || (msg.kind == 'KEY_DOWN' && msg.key == 'ESCAPE')) {
        break;
    }

    $actor_sleep(16);
}
