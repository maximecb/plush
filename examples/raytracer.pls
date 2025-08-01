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
    }

    // The color is specified as an u32 value in RGBA32 format
    set_pixel(self, x, y, color)
    {
        let idx = 4 * (y * self.width + x);
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

        if (dst_x < 0)
        {
            src_x = -dst_x;
            width = width + dst_x;
            dst_x = 0;
        }

        if (dst_y < 0)
        {
            src_y = -dst_y;
            height = height + dst_y;
            dst_y = 0;
        }

        if (dst_x + width > self.width)
        {
            width = self.width - dst_x;
        }

        if (dst_y + height > self.height)
        {
            height = self.height - dst_y;
        }

        if (width <= 0 || height <= 0)
        {
            return;
        }

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

// Vector class for 3D points and vectors
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

    // Length squared
    length_squared(self) {
        return self.dot(self);
    }

    // Normalize vector
    normalize(self) {
        let len = self.length_squared().sqrt();
        return Vec3(self.x / len, self.y / len, self.z / len);
    }
}

// Ray class
class Ray {
    init(self, origin, direction) {
        self.origin = origin;
        self.direction = direction;
    }

    // Point at parameter t
    at(self, t) {
        return self.origin.add(self.direction.mul(t));
    }
}

// Sphere class
class Sphere {
    init(self, center, radius) {
        self.center = center;
        self.radius = radius;
        self.radius_sq = radius * radius;
    }

    // Ray-sphere intersection, returns only the distance t
    hit(self, ray, t_min, t_max) {
        let oc = ray.origin.sub(self.center);
        let a = ray.direction.length_squared();
        let half_b = oc.dot(ray.direction);
        let c = oc.length_squared() - self.radius_sq;
        let discriminant = half_b * half_b - a * c;

        if (discriminant < 0) {
            return nil;
        }

        let sqrtd = discriminant.sqrt();
        let inv_a = 1.0 / a;

        // Find the nearest root that lies in the acceptable range
        let var t = (-half_b - sqrtd) * inv_a;
        if (t > t_min && t < t_max) {
            return t;
        }

        t = (-half_b + sqrtd) * inv_a;
        if (t > t_min && t < t_max) {
            return t;
        }

        return nil;
    }
}

// Simple diffuse material
class Material {
    init(self, color) {
        self.color = color;
    }

    shade(self, hit_point, normal, light_pos) {
        let light_dir = light_pos.sub(hit_point).normalize();
        let diffuse = max(0, light_dir.dot(normal));
        return self.color.mul(diffuse);
    }
}

// Camera class
class Camera {
    init(self, width, height) {
        self.width = width;
        self.height = height;
        let aspect_ratio = width / height;

        // Camera setup
        let viewport_height = 2.0;
        let viewport_width = aspect_ratio * viewport_height;
        let focal_length = 1.0;

        self.origin = Vec3(0, 0, 0);
        let horizontal = Vec3(viewport_width, 0, 0);
        let vertical = Vec3(0, viewport_height, 0);
        let top_left_corner = self.origin.sub(horizontal.mul(0.5))
                                     .add(vertical.mul(0.5))
                                     .sub(Vec3(0, 0, focal_length));

        self.u_vec = horizontal.mul(1.0 / (width - 1));
        self.v_vec = vertical.mul(1.0 / (height - 1));
        self.base_dir = top_left_corner.sub(self.origin);
    }
}

// Scene class to hold the objects to be rendered
class Scene {
    init(self) {
        self.sphere = Sphere(Vec3(0, 0, -1), 0.5);
        self.material = Material(Vec3(1, 0, 0)); // Red sphere
        self.light_pos = Vec3(2, 2, 1);
    }

    hit(self, ray, t_min, t_max) {
        return self.sphere.hit(ray, t_min, t_max);
    }

    shade(self, hit_point, normal) {
        return self.material.shade(hit_point, normal, self.light_pos);
    }
}

// Renders a tile of the image
fun render_tile(scene, camera, xmin, ymin, xmax, ymax) {
    let tile_w = xmax - xmin;
    let tile_h = ymax - ymin;
    let tile_img = Image(tile_w, tile_h);

    let tile_start_dir = camera.base_dir.add(camera.u_vec.mul(xmin)).sub(camera.v_vec.mul(ymin));

    for (let var j = 0; j < tile_h; ++j) {
        let row_start_dir = tile_start_dir.sub(camera.v_vec.mul(j));

        for (let var i = 0; i < tile_w; ++i) {
            let dir = row_start_dir.add(camera.u_vec.mul(i));
            let ray = Ray(camera.origin, dir);

            let t = scene.hit(ray, 0.001, 1000);
            if (t != nil) {
                let point = ray.at(t);
                let normal = point.sub(scene.sphere.center).normalize();
                let color = scene.shade(point, normal);

                let ir = (255.999 * color.x).floor();
                let ig = (255.999 * color.y).floor();
                let ib = (255.999 * color.z).floor();
                tile_img.set_pixel(i, j, rgb32(ir, ig, ib));
            }

        }
    }

    return tile_img;
}

class RenderRequest
{
    init(self, scene, camera, xmin, ymin, xmax, ymax, x, y)
    {
        self.scene = scene;
        self.camera = camera;
        self.xmin = xmin;
        self.ymin = ymin;
        self.xmax = xmax;
        self.ymax = ymax;
        self.x = x;
        self.y = y;
    }
}

class RenderResult
{
    init(self, tile_img, x, y, actor_id)
    {
        self.tile_img = tile_img;
        self.x = x;
        self.y = y;
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

        let tile_img = render_tile(
            msg.scene,
            msg.camera,
            msg.xmin,
            msg.ymin,
            msg.xmax,
            msg.ymax
        );

        let result = RenderResult(tile_img, msg.x, msg.y, $actor_id());
        $actor_send($actor_parent(), result);
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
    let width = 400;
    let height = 300;

    // Camera setup
    let camera = Camera(width, height);

    // Scene setup
    let scene = Scene();

    // Create a list of tile requests to render
    let requests = [];
    for (let var y = 0; y < height; y = y + TILE_SIZE) {
        for (let var x = 0; x < width; x = x + TILE_SIZE) {
            let xmax = min(x + TILE_SIZE, width);
            let ymax = min(y + TILE_SIZE, height);
            requests.push(RenderRequest(scene, camera, x, y, xmax, ymax, x, y));
        }
    }
    let num_tiles = requests.len;

    // Image to render into
    let image = Image(width, height);

    let start_time = $time_current_ms();

    // Send one requests to each actor, round-robin
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

        image.blit(msg.tile_img, msg.x, msg.y);
    }

    let render_time = $time_current_ms() - start_time;
    $println("Render time: " + render_time.to_s() + "ms");

    // Tell actors to terminate
    for (let var i = 0; i < num_actors; ++i)
    {
        $actor_send(actor_ids[i], nil);
    }

    return image;
}

// Single-threaded rendering
fun render_no_tile()
{
    // Image settings
    let width = 400;
    let height = 300;

    // Camera setup
    let camera = Camera(width, height);

    // Scene setup
    let scene = Scene();

    return render_tile(scene, camera, 0, 0, width, height);
}

// Run the renderer
//let image = render_no_tile();
let image = render();

let window = $window_create(image.width, image.height, "Render", 0);
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
