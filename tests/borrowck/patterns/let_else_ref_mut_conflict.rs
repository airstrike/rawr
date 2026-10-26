fn main() {
    let mut o = Some(vec![1]);
    let Some(ref mut x) = o else {
        return;
    };
    let c = o.is_some();
    x.push(2);
    println!("{:?} {}", o, c);
}
