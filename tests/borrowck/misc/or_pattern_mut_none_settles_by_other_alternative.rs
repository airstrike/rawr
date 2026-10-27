fn main() {
    let mut p = (None, Some(2));
    match &mut p {
        (Some(x), _) | (None, Some(x)) => *x += 1,
        _ => {}
    }
    println!("{p:?}");
}
