struct Pair {
    left: Vec<i32>,
    right: Vec<i32>,
}

fn main() {
    let mut pair = Pair { left: vec![1], right: vec![2] };
    let mut r = &mut pair.left;
    r.push(3);
    r = &mut pair.right;
    r.push(4);
    println!("{:?} {:?}", pair.left, pair.right);
}
