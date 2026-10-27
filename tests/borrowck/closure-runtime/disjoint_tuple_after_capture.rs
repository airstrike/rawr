fn main() {
    let mut pair = (vec![1, 2, 3, 4], vec![2]);
    let mut push = |x| pair.1.push(x);
    push(3);
    pair.0.retain(|x| !pair.1.contains(x));
    pair.0.iter_mut().for_each(|x| *x += pair.1.len() as i32);
    println!("{:?} {:?}", pair.0, pair.1);
}
