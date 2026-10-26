fn get<T: Copy>(x: &T) -> T {
    *x
}

fn main() {
    let mut a = 1;
    let b = 2;
    let r = &b;
    let copied = get(&r);
    a += 1;
    println!("{a} {copied}");
}
