struct Pair<'a> {
    left: &'a i32,
    right: &'a i32,
}

fn main() {
    let a = 1;
    let mut b = 2;
    let c = 3;
    let mut pair = Pair { left: &a, right: &b };
    pair.right = &c;
    b += 1;
    println!("{} {} {b}", pair.left, pair.right);
}
