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
            //self.bytes.fill_u32(offset, width, color);
        }
    }
}

let img = new Image(800, 600);
$println(img.width);
$println(img.height);

img.set_pixel(400, 300, 0xFF_FF_FF_FF);
img.fill_rect(100, 100, 300, 200, 0xFF_FF_FF_FF);
