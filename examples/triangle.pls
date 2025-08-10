let TILE_SIZE = 30;

// Convert RGB/RGBA values in the range [0, 255] to a u32 encoding
fun rgb32(r, g, b)
{
    return 0xFF_00_00_00 | (r << 16) | (g << 8) | b;
}

class Image
{
    init(self, width, height)
    {
        assert(width instanceof Int64);
        assert(height instanceof Int64);

        self.width = width;
        self.height = height;
        self.bytes = ByteArray.with_size(4 * width * height);
        
        // Initialize with black background
        for (let var i = 0; i < width * height; ++i) {
            self.bytes.write_u32(i, rgb32(0, 0, 0));
        }
    }

    // The color is specified as an u32 value in RGBA32 format
    set_pixel(self, x, y, color)
    {
        let idx = y * self.width + x;
        self.bytes.write_u32(idx, color);
    }

    // Copy a source image into this image at a given position
    blit(self, src_img, dst_x, dst_y)
    {
        let var dst_x = dst_x;
        let var dst_y = dst_y;
        let var src_x = 0;
        let var src_y = 0;
        let var width = src_img.width;
        let var height = src_img.height;

        // Number of bytes per row of the images
        let dst_pitch = self.width * 4;
        let src_pitch = src_img.width * 4;

        for (let var j = 0; j < height; ++j)
        {
            let src_idx = (src_y + j) * src_pitch + src_x * 4;
            let dst_idx = (dst_y + j) * dst_pitch + dst_x * 4;
            self.bytes.memcpy(dst_idx, src_img.bytes, src_idx, width * 4);
        }
    }
}

fun max(a, b) {
    if (a > b) return a;
    return b;
}

fun min(a, b) {
    if (a < b) return a;
    return b;
}

// 2D Vector class for screen coordinates
class Vec2 {
    init(self, x, y) {
        self.x = x;
        self.y = y;
    }

    add(self, other) {
        return Vec2(self.x + other.x, self.y + other.y);
    }

    sub(self, other) {
        return Vec2(self.x - other.x, self.y - other.y);
    }

    mul(self, scalar) {
        return Vec2(self.x * scalar, self.y * scalar);
    }
}

// 3D Vector class for colors and 3D positions
class Vec3 {
    init(self, x, y, z) {
        self.x = x;
        self.y = y;
        self.z = z;
    }

    add(self, other) {
        return Vec3(self.x + other.x, self.y + other.y, self.z + other.z);
    }

    sub(self, other) {
        return Vec3(self.x - other.x, self.y - other.y, self.z - other.z);
    }

    mul(self, scalar) {
        return Vec3(self.x * scalar, self.y * scalar, self.z * scalar);
    }

    dot(self, other) {
        return self.x * other.x + self.y * other.y + self.z * other.z;
    }
}

// Vertex class with position and color
class Vertex {
    init(self, pos, color) {
        self.pos = pos;  // Vec2 screen position
        self.color = color;  // Vec3 RGB color (0-1 range)
    }
}

// Triangle class
class Triangle {
    init(self, v0, v1, v2) {
        self.v0 = v0; 
        self.v1 = v1;
        self.v2 = v2;
    }

    dot(self, v, w) {
        return(v.x*w.x + v.y*w.y);
    }

    // Calculate barycentric coordinates for point p
    barycentric(self, p) {
        let v0 = self.v2.pos.sub(self.v0.pos);
        let v1 = self.v1.pos.sub(self.v0.pos);
        let v2 = p.sub(self.v0.pos);

        let dot00 = self.dot(v0, v0);
        let dot01 = self.dot(v0, v1);
        let dot02 = self.dot(v0, v2);
        let dot11 = self.dot(v1, v1);
        let dot12 = self.dot(v1, v2);

        let inv_denom = 1 / (dot00 * dot11 - dot01 * dot01);
        let u = (dot11 * dot02 - dot01 * dot12) * inv_denom;
        let v = (dot00 * dot12 - dot01 * dot02) * inv_denom;

        return Vec3(1 - u - v, v, u);  // (w0, w1, w2)
    }

    // Check if point is inside triangle using barycentric coordinates
    is_inside(self, p) {
        let bary = self.barycentric(p);
        return bary.x >= 0 && bary.y >= 0 && bary.z >= 0;
    }

    // Interpolate color at point p using barycentric coordinates
    interpolate_color(self, p) {
        let bary = self.barycentric(p);
        let color = self.v0.color.mul(bary.x)
                    .add(self.v1.color.mul(bary.y))
                    .add(self.v2.color.mul(bary.z));
        return color;
    }
}

// Renders a tile of the image
fun render_tile(scene, xmin, ymin) {
    let tile_img = Image(TILE_SIZE, TILE_SIZE);
    for (let var y = 0; y < TILE_SIZE; ++y) {
        for (let var x = 0; x < TILE_SIZE; ++x) {
            let screen_x = xmin + x;
            let screen_y = ymin + y;
            let p = Vec2(screen_x, screen_y);

            // Test all triangles
            for (let var t = 0; t < scene.triangles.len; ++t) {
                let triangle = scene.triangles[t];
                
                if (triangle.is_inside(p)) {
                    let color = triangle.interpolate_color(p);
                    
                    // Convert to RGB integers
                    let ir = min(255, max(0, (255 * color.x).floor()));
                    let ig = min(255, max(0, (255 * color.y).floor()));
                    let ib = min(255, max(0, (255 * color.z).floor()));
                    
                    tile_img.set_pixel(x, y, rgb32(ir, ig, ib));
                    break; // First triangle wins (no depth testing)
                }
            }
        }
    }
    return tile_img;
}

class RenderRequest
{
    init(self, scene, xmin, ymin)
    {
        self.scene = scene;
        self.xmin = xmin;
        self.ymin = ymin;
    }
}

class RenderResult
{
    init(self, tile_img, xmin, ymin, actor_id)
    {
        self.tile_img = tile_img;
        self.xmin = xmin;
        self.ymin = ymin;
        self.actor_id = actor_id;
    }
}

fun actor_loop()
{
    while (true)
    {
        let msg = $actor_recv();

        // Done rendering
        if (msg == nil)
            return;

        // Render the tile directly, let render_tile compute bounds
        let tile_img = render_tile(
            msg.scene,
            msg.xmin,
            msg.ymin
        );

        let result = RenderResult(tile_img, msg.xmin, msg.ymin, $actor_id());
        $actor_send($actor_parent(), result);
    }
}

// Scene class to hold triangles to be rendered
class Scene {
    init(self) {
        self.triangles = [];
        self.setup_triangles();
    }

    setup_triangles(self) {
        let v0 = Vertex(Vec2(300, 100), Vec3(1, 0, 0));   // Red
        let v1 = Vertex(Vec2(60, 540), Vec3(0, 1, 0));  // Green  
        let v2 = Vertex(Vec2(540, 540), Vec3(0, 0, 1));  // Blue
        self.triangles.push(Triangle(v0, v1, v2));
    }
}

// Multi-actor, parallel rendering
fun render()
{
    let num_actors = 32;

    // Create the actors
    let actor_ids = [];
    for (let var i = 0; i < num_actors; ++i)
        actor_ids.push($actor_spawn(actor_loop));

    // Image settings
    let width = 600;
    let height = 600;

    // Scene setup
    let scene = Scene();

    // Create a list of tile requests to render
    let requests = [];
    
    for (let var y = 0; y < height; y = y + TILE_SIZE) {
        for (let var x = 0; x < width; x = x + TILE_SIZE) {
            requests.push(RenderRequest(scene, x, y));
        }
    }

    let num_tiles = requests.len;

    // Image to render into (base)
    let image = Image(width, height);

    let start_time = $time_current_ms();

    // Send one request to each actor
    for (let var i = 0; i < num_actors; ++i)
    {
        $actor_send(actor_ids[i % num_actors], requests.pop());
    }

    // Receive all the render results
    for (let var num_received = 0; num_received < num_tiles; ++num_received)
    {
        let msg = $actor_recv();

        // Send more work to this actor, since it is no longer busy
        if (requests.len > 0)
        {
            $actor_send(msg.actor_id, requests.pop());
        }

        image.blit(msg.tile_img, msg.xmin, msg.ymin);
    }

    let render_time = $time_current_ms() - start_time;
    $println("Parallel render time: " + render_time.to_s() + "ms");

    return image;
}

// Run the renderer
let image = render();

let window = $window_create(image.width, image.height, "Triangle Rasterizer", 0);
$window_draw_frame(window, image.bytes);

loop
{
    let msg = $actor_recv();

    if (!(msg instanceof UIEvent))
        continue;

    if (msg.kind == 'CLOSE_WINDOW')
        break;

    if (msg.kind == 'KEY_DOWN' && msg.key == 'ESCAPE')
        break;
}