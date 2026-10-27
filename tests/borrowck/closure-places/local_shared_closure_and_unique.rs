fn main() {
    let mut v = vec![1];
    let r = &mut v;
    let g = || r.len();
    let mut f = || r.push(2);
    f();
    println!("{}", g());
}
