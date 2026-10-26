fn add(a: &Vec<i64>, b: usize) -> usize {
    a.len() + b
}
fn grow(v: &mut Vec<i64>) -> usize {
    v.push(1);
    v.len()
}
fn main() {
    let mut v = vec![1];
    let g = grow(&mut v);
    let n = add(&v, g);
    println!("{n}");
}
