fn add(a: &Vec<i64>, b: usize) -> usize {
    a.len() + b
}
fn grow(v: &mut Vec<i64>) -> usize {
    v.push(1);
    v.len()
}
fn main() {
    let mut v = vec![1];
    let n = add(&v, grow(&mut v));
    println!("{n}");
}
