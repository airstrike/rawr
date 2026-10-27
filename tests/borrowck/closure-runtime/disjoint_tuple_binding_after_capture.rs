fn main() {
    let mut pairs = vec![(vec![1, 2, 3], vec![1])];
    if let Some(pair) = pairs.get_mut(0) {
        let mut push = |x| pair.1.push(x);
        push(3);
        pair.0.retain(|x| !pair.1.contains(x));
    }
    for pair in &mut pairs {
        pair.0.iter_mut().for_each(|x| *x += pair.1.len() as i32);
    }
    println!("{:?}", pairs);
}
