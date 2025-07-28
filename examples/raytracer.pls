// Convert RGB/RGBA values in the range [0, 255] to a u32 encoding
fun rgb32(r, g, b)
{
    return 0xFF_00_00_00 + (r * 65536) + (g * 256) + b;
}

class Image
{
    init(self, width, height)
    {
        self.width = width;
        self.height = height;
        self.bytes = ByteArray.with_size(4 * width * height);
    }

    // The color is specified as an u32 value in RGBA32 format
    set_pixel(self, x, y, color)
    {
        self.bytes.write_u32(4 * (y * self.width + x), color);
    }
}

fun max(a, b) {
    if (a > b) return a;
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
        return new Vec3(self.x + other.x, self.y + other.y, self.z + other.z);
    }

    // Vector subtraction
    sub(self, other) {
        return new Vec3(self.x - other.x, self.y - other.y, self.z - other.z);
    }

    // Scalar multiplication
    mul(self, scalar) {
        return new Vec3(self.x * scalar, self.y * scalar, self.z * scalar);
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
        return new Vec3(self.x / len, self.y / len, self.z / len);
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
    }

    // Ray-sphere intersection, returns only the distance t
    hit(self, ray, t_min, t_max) {
        let oc = ray.origin.sub(self.center);
        let a = ray.direction.length_squared();
        let half_b = oc.dot(ray.direction);
        let c = oc.length_squared() - self.radius * self.radius;
        let discriminant = half_b * half_b - a * c;

        if (discriminant < 0) {
            return nil;
        }

        let sqrtd = discriminant.sqrt();
        let var t = (-half_b - sqrtd) / a;
        if (t < t_min || t > t_max) {
            t = (-half_b + sqrtd) / a;
            if (t < t_min || t > t_max) {
                return nil;
            }
        }

        return t;
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

// Main rendering function
fun render()
{
    // Image settings
    let width = 256;
    let height = 256;
    let aspect_ratio = width / height;

    // Camera setup
    let viewport_height = 2.0;
    let viewport_width = aspect_ratio * viewport_height;
    let focal_length = 1.0;

    let origin = new Vec3(0, 0, 0);
    let horizontal = new Vec3(viewport_width, 0, 0);
    let vertical = new Vec3(0, viewport_height, 0);
    let top_left_corner = origin.sub(horizontal.mul(0.5))
                                .add(vertical.mul(0.5))
                                .sub(new Vec3(0, 0, focal_length));

    // Scene setup
    let sphere = new Sphere(new Vec3(0, 0, -1), 0.5);
    let material = new Material(new Vec3(1, 0, 0)); // Red sphere
    let light_pos = new Vec3(2, 2, 0);

    let window = $window_create(width, height, "Render", 0);
    let image = new Image(width, height);

    // Render loop
    for (let var j = 0; j < height; j = j + 1)
    {
        //$println(j.to_s());

        for (let var i = 0; i < width; i = i + 1)
        {
            let u = i / (width - 1);
            let v = j / (height - 1);

            let ray = new Ray(origin, top_left_corner.add(horizontal.mul(u)).sub(vertical.mul(v)).sub(origin));
            let var color = new Vec3(0, 0, 0); // Black background

            let t = sphere.hit(ray, 0.001, 1000);
            if (t != nil) {
                let point = ray.at(t);
                let normal = point.sub(sphere.center).normalize();
                color = material.shade(point, normal, light_pos);
                //color = new Vec3(1, 1, 1);
            }

            // Output color as integers [0, 255]
            let ir = (255.999 * color.x).floor();
            let ig = (255.999 * color.y).floor();
            let ib = (255.999 * color.z).floor();
            image.set_pixel(i, j, rgb32(ir, ig, ib));
        }
    }

    $window_draw_frame(window, image.bytes);

    while (true)
    {
        let msg = $actor_recv();

        if (!(msg instanceof UIMessage))
        {
            continue;
        }

        if (msg.event == 'CLOSE_WINDOW')
        {
            break;
        }

        if (msg.event == 'KEY_DOWN' && msg.key == 'ESCAPE')
        {
            break;
        }
    }
}

// Run the renderer
render();
