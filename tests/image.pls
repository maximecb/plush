let COLOR_BLACK     = 0xFF_00_00_00;
let COLOR_WHITE     = 0xFF_FF_FF_FF;
let COLOR_GREY      = 0xFF_80_80_80;
let COLOR_RED       = 0xFF_FF_00_00;
let COLOR_GREEN     = 0xFF_00_FF_00;
let COLOR_BLUE      = 0xFF_00_00_FF;
let COLOR_ORANGE    = 0xFF_FF_A5_00;
let COLOR_YELLOW    = 0xFF_FF_FF_00;
let COLOR_MAGENTA   = 0xFF_FF_00_FF;
let COLOR_PURPLE    = 0xFF_D6_00_FF;
let COLOR_TURQUOISE = 0xFF_40_E0_D0;

// Convert RGB/RGBA values in the range [0, 255] to a u32 encoding
//#define rgb32(r, g, b) ((u32)0xFF_00_00_00 | ((u32)r << 16) | ((u32)g << 8) | (u32)b)
//#define rgba32(r, g, b, a) (((u32)a << 24) | ((u32)r << 16) | ((u32)g << 8) | (u32)b)

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

    // Fill a rectangle area with a given color
    fill_rect(
        self,
        xmin,
        ymin,
        width,
        height,
        color
    )
    {
        for (let var j = 0; j < height; ++j)
        {
            let offset = 4 * (self.width * (ymin + j) + xmin);
            self.bytes.fill_u32(offset, width, color);
        }
    }
}

let img = new Image(800, 600);
$println(img.width);
$println(img.height);

img.set_pixel(100, 50, COLOR_BLUE);

img.fill_rect(100, 100, 200, 100, COLOR_BLUE);
img.fill_rect(150, 150, 250, 200, COLOR_RED);




/*
let window = $window_create(800, 600, "Test window", 0);
$window_draw_frame(window, img.bytes);

while (true)
{
    $actor_recv();
}
*/
