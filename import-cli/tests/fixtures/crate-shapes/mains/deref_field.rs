// rawr
use natives::deref::{Bytes, Shot};

fn save(bytes: &[u8], w: u32, h: u32) -> usize {
    bytes.len() + (w + h) as usize
}

fn main() {
    let shot = Shot::new();
    println!("{}", save(&shot.rgba, shot.size.width, shot.size.height));
    let r: &[u8] = &shot.rgba;
    println!("{} {}", r.len(), shot.size.width);
    let bytes: &Bytes = &shot.rgba;
    println!("{} {:?}", bytes.len(), bytes.first());
    println!("{}", shot.rgba.len());
}
