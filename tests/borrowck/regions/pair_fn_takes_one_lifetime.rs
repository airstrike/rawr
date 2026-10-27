struct Pair<'a, 'b> {
    x: &'a i32,
    y: &'b i32,
}

fn make<'a, 'b>(x: &'a i32, y: &'b i32) -> Pair<'a, 'b> {
    Pair { x, y }
}

fn main() {
    let a = 1;
    let mut b = 2;
    let p = make(&a, &b);
    let x = p.x;
    b += 1;
    println!("{x} {b}");
}
