fn main() {
    let mut pairs: Vec<(Option<i64>, Option<i64>)> = vec![(Some(1), None), (None, Some(2)), (None, None)];
    for pair in &mut pairs {
        match pair {
            (Some(x), _) | (None, Some(x)) => *x += 10,
            _ => {}
        }
    }
    println!("{pairs:?}");
}
