fn make<'a>(v: &'a Vec<i32>) -> impl Fn() -> usize + 'a {
    || v.len()
}
fn main() {
    let v = vec![1];
    let f = make(&v);
    println!("{}", f());
}
