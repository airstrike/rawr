fn make() -> impl Fn() -> usize {
    let v = vec![1];
    || v.len()
}
fn main() {
    let f = make();
    println!("{}", f());
}
