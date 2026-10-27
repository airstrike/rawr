fn main() {
    let mut v = Some(vec![0]);
    if let Some(x) = &mut v {
        let v = 1;
        x.push(v);
    }
    println!("{v:?}");
}
