// Test ByteArray manipulation functions with direct byte access

// Test with_size
let b1 = ByteArray.with_size(16);
assert(b1.len == 16);

// Test write_u32
// Assumes little-endian byte order for the host system
let b2 = ByteArray.with_size(4);
b2.write_u32(0, 0x12345678);
assert(b2[0] == 0x78);
assert(b2[1] == 0x56);
assert(b2[2] == 0x34);
assert(b2[3] == 0x12);

// Test fill_u32
let b3 = ByteArray.with_size(8);
b3.fill_u32(0, 2, 0xDEADBEEF);
assert(b3[0] == 0xEF);
assert(b3[1] == 0xBE);
assert(b3[2] == 0xAD);
assert(b3[3] == 0xDE);
assert(b3[4] == 0xEF);
assert(b3[5] == 0xBE);
assert(b3[6] == 0xAD);
assert(b3[7] == 0xDE);

// Test zero_fill
let b4 = ByteArray.with_size(4);
b4.fill_u32(0, 1, 0xFFFFFFFF);
b4.zero_fill();
assert(b4[0] == 0);
assert(b4[1] == 0);
assert(b4[2] == 0);
assert(b4[3] == 0);

// Test memcpy
let b5_src = ByteArray.with_size(4);
b5_src.write_u32(0, 0xCAFEBABE);

let b5_dst = ByteArray.with_size(4);
b5_dst.memcpy(0, b5_src, 0, 4);

assert(b5_dst[0] == 0xBE);
assert(b5_dst[1] == 0xBA);
assert(b5_dst[2] == 0xFE);
assert(b5_dst[3] == 0xCA);

$println("ByteArray tests passed!");