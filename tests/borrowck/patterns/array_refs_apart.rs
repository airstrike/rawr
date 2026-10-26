fn main() {
    let mut arr = [1, 2, 3];
    let [ref mut a, .., ref mut b] = arr;
    *a += 10;
    *b += 20;
    println!("{:?}", arr);
}
