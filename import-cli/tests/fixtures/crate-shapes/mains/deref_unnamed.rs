// rawr
// As deref_field.rs, but the program names no path to the field's type:
// iced's screenshot reaches `Bytes` only as `Screenshot::rgba`'s.
use natives::deref::Shot;

fn save(bytes: &[u8], w: u32, h: u32) -> usize {
    bytes.len() + (w + h) as usize
}

fn main() {
    let shot = Shot::new();
    println!("{}", save(&shot.rgba, shot.size.width, shot.size.height));
}
