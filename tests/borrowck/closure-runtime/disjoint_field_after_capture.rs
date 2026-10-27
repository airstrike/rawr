struct Pair {
    a: Vec<i32>,
    b: Vec<i32>,
}

fn main() {
    let mut pair = Pair { a: vec![1, 2], b: vec![10] };
    let count = || pair.b.len();
    println!("{}", count());
    pair.a.iter_mut().for_each(|x| *x += pair.b[0]);
    pair.b.push(20);
    pair.a.sort_by_key(|x| pair.b.len() as i32 - *x);
    println!("{:?} {:?}", pair.a, pair.b);
}
