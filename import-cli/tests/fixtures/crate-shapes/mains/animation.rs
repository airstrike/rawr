// rawr
use natives::animation::Animation;

fn main() {
    // rustc types `0.` as `f32`: only `f32` of the float types is `Float`.
    let animation = Animation::new(0.).slow();
    println!("{} {}", animation.kind(), animation.value());
    let made = || Animation::new(0.25);
    println!("{} {}", made().kind(), made().value());
}
