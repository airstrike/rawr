struct Lists {
    keep: Vec<i32>,
    drop: Vec<i32>,
}

fn main() {
    let mut lists = Lists { keep: vec![1, 2, 3, 4, 5], drop: vec![2, 4] };
    let __b = &mut lists;
    for __i in 0..2 {
        __touch(__b);
        lists.keep.retain(|x| !lists.drop.contains(x));
    }
    println!("{:?} {:?}", lists.keep, lists.drop);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
