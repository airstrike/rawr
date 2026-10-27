struct Pair<'a, 'b> {
    x: &'a i32,
    y: &'b i32,
}

fn make<'a, 'b>(x: &'a i32, y: &'b i32) -> Pair<'a, 'b> {
    Pair { x, y }
}

fn main() {
    let mut a = 1;
    let b = 2;
    let p = make(&a, &b);
    let x = p.x;
    a += 1;
    println!("{x}");
}
