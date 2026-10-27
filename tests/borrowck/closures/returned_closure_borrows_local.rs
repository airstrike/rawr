fn make() -> Box<dyn Fn() -> usize> {
    let v = vec![1];
    Box::new(|| v.len())
}
fn main() {
    let f = make();
    println!("{}", f());
}
