fn main() {
    let mut v = vec![vec![1], vec![2]];
    for i in 0..3 {
        match v.get_mut(i) {
            Some(inner) => inner.push(i as i64),
            None => println!("none at {i}"),
        }
    }
    while let Some(last) = v.last_mut() {
        if last.len() > 3 {
            break;
        }
        last.push(0);
    }
    println!("{v:?}");
}
