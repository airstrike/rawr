fn main() {
    let mut o = Some(vec![1]);
    let Some(ref mut x) = o else {
        return;
    };
    x.push(2);
    let c = o.is_some();
    println!("{:?} {}", o, c);
}
