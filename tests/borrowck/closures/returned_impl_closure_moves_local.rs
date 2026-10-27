fn make() -> impl Fn() -> usize {
    let v = vec![1];
    move || v.len()
}
fn main() {
    let f = make();
    println!("{}", f());
}
