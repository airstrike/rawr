fn id<T>(x: &T) -> &T {
    x
}

fn main() {
    let mut v = vec![1];
    let mut keep = &v;
    println!("{keep:?}");
    v.push(2);
    keep = &v;
    let c = std::env::args().count() > 5;
    let t = if c { id(&keep) } else { id(&keep) };
    println!("{t:?}");
}
