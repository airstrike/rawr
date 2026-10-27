fn bump(r: &mut i32) {
    let c = || *r += 1;
    let mut d = c;
    d();
    d();
}

fn main() {
    let mut n = 0;
    bump(&mut n);
    println!("{}", n);
}
