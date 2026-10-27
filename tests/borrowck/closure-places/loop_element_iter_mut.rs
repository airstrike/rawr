fn main() {
    let mut v = vec![vec![1], vec![2]];
    for (i, list) in v.iter_mut().enumerate() {
        let mut push = |x| list.push(x);
        push(i as i32 * 10);
    }
    println!("{v:?}");
}
