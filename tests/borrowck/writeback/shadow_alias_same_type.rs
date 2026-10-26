fn show(v: &Vec<i64>) {
    println!("{v:?}");
}
fn main() {
    let mut b = vec![0];
    let r = &mut b;
    let b = vec![7];
    r.push(1);
    r.push(2);
    show(&b);
    println!("{:?}", r);
}
