fn main() {
    let mut x = 1;
    let mut y = 10;
    let mut z = 100;
    let mut r = &mut x;
    *r += 1;
    r = &mut y;
    *r *= 3;
    r = &mut z;
    *r = 7;
    println!("{x} {y} {z}");
}
