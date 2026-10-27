fn main() {
    let pair = (None, 1);
    let first = pair;
    let second = pair;
    let typed: (Option<u8>, i32) = first;
    println!("{:?} {:?}", typed, second);
}
