struct Pair<'a, 'b> {
    x: &'a i32,
    y: &'b i32,
}

fn main() {
    let a = 1;
    let mut b = 2;
    let v = vec![Pair { x: &a, y: &b }];
    let x = v[0].x;
    b += 1;
    println!("{x} {b}");
}
