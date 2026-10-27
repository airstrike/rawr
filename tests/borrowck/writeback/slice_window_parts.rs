fn main() {
    let mut v = vec![1, 2, 3, 4, 5];
    if let [a, .., b] = &mut v[1..4] {
        *a += 10;
        *b += 20;
    }
    match &mut v[..] {
        [first, .., last] => {
            *first = *last * 2;
        }
        _ => {}
    }
    println!("{v:?}");
}
