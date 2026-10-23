let mut pixels = vec![0u8; 64 * 64 * 4];
for y in 0..64 {
    for x in 0..64 {
        let i = (y * 64 + x) * 4;
        pixels[i] = (x * 4) as u8;
        pixels[i + 1] = (y * 4) as u8;
        pixels[i + 2] = 160;
        pixels[i + 3] = 255;
    }
}
image(image::Handle::from_rgba(64, 64, pixels)).width(64).height(64)
