fn call<F: FnMut()>(mut f: F) {
    f();
}
fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    call(|| v.push(3));
    println!("{:?}", r);
}
