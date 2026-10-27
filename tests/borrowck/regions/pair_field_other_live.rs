struct Pair<'a, 'b> {
    x: &'a i32,
    y: &'b i32,
}

fn main() {
    let a = 1;
    let mut b = 2;
    let p = Pair { x: &a, y: &b };
    let y = p.y;
    b += 1;
    println!("{y} {b}");
}
