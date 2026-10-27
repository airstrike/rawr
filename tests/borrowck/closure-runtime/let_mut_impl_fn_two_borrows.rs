fn make() -> impl Fn() -> usize {
    || 7
}

fn both(a: &dyn Fn() -> usize, b: usize) -> usize {
    a() + b
}

fn main() {
    let mut f = make();
    println!("{}", both(&f, f()));
    f = make();
    println!("{}", f());
}
