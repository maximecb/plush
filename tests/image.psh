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
        self.bytes.write_u32(y * self.width + x, color);
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
            let offset = self.width * (ymin + j) + xmin;
            self.bytes.fill_u32(offset, width, color);
        }
    }
}

let img = Image(800, 600);
$println(img.width);
$println(img.height);

img.set_pixel(100, 50, COLOR_BLUE);

img.fill_rect(100, 100, 200, 100, COLOR_BLUE);
img.fill_rect(150, 150, 250, 200, COLOR_RED);
