fn main() {
    let mut arr = [1, 2, 3];
    let [ref mut a, ..] = arr;
    arr[1] = 5;
    *a += 10;
    println!("{:?}", arr);
}
