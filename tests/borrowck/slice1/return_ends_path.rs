fn f(c: bool) -> usize {
    let mut v = vec![1];
    let r = &mut v;
    if c {
        v.push(2);
        return v.len();
    }
    r.push(3);
    v.len()
}
fn main() {
    println!("{} {}", f(true), f(false));
}
