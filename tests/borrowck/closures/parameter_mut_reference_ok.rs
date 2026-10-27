fn grow(v: &mut Vec<i32>) {
    let mut c = || v.push(1);
    c();
    c();
    println!("{}", v.len());
}
fn main() {
    let mut v = vec![1];
    grow(&mut v);
    println!("{:?}", v);
}
