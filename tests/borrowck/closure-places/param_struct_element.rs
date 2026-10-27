fn bump(r: &mut Vec<i32>) {
    let mut push = |x| r.push(x);
    push(1);
}

struct Holder {
    lists: Vec<Vec<i32>>,
}

fn main() {
    let mut h = Holder { lists: vec![vec![0], vec![5]] };
    bump(&mut h.lists[1]);
    let copy = h.lists.clone();
    h.lists[1].push(7);
    println!("{:?} {:?}", h.lists, copy);
}
