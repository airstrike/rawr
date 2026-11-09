// rawr
//! A marked leaf that makes a native picture's pixels and hands them
//! `&mut` to another marked function, which writes them through a native
//! `&mut` method, as inboard's `make_circle_avatar` does on a tokio worker.

/// The picture `bytes` hold, its corners cut out.
pub fn circle(bytes: Vec<u8>) -> Option<Vec<u8>> {
    let picture = ui::load(&bytes).ok()?;
    let mut rgba = picture.to_rgba();
    mask(&mut rgba);
    Some(rgba.into_raw())
}

fn mask(rgba: &mut ui::Rgba) {
    let (width, height) = rgba.dimensions();
    for y in 0..height {
        for x in 0..width {
            let corner = (x == 0 || x == width - 1) && (y == 0 || y == height - 1);
            if corner {
                rgba.get_pixel_mut(x, y).0 = [0; 4];
            } else {
                rgba.get_pixel_mut(x, y).0[3] = (x + y) as u8;
            }
        }
    }
}
