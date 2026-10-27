struct Pair<'a, 'b> {
    x: &'a i32,
    y: &'b i32,
}

fn second<'a, 'b>(p: &Pair<'a, 'b>) -> &'b i32 {
    p.y
}

fn main() {
    let a = 1;
    let mut b = 2;
    let p = Pair { x: &a, y: &b };
    let y = second(&p);
    b += 1;
    println!("{y}");
}
