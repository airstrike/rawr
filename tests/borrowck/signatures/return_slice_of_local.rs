fn tail() -> &'static [i32] {
    let v = vec![1, 2, 3];
    &v[1..]
}

fn main() {
    println!("{:?}", tail());
}
