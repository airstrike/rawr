// rawr
// A public field of a struct the crate implements `Drop` for is read where
// it is, as integration's `frame.texture.create_view(..)` reads wgpu's
// `SurfaceTexture::texture`.
use natives::frames::acquire;

fn main() {
    let frame = acquire(5);
    println!("{}", frame.texture.id());
}
