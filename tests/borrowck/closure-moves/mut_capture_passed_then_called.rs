fn call(mut f: impl FnMut()) {
    f();
}

fn main() {
    let mut n = 0;
    let mut c = || n += 1;
    call(c);
    c();
    println!("{}", n);
}
