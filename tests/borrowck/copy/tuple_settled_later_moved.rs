fn main() {
    let pair = (None, 1);
    let first = pair;
    let second = pair;
    let typed: (Option<String>, i32) = first;
    println!("{:?} {:?}", typed, second);
}
