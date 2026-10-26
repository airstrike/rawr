struct T {
    field: Vec<i64>,
    other: i64,
}
fn main() {
    let mut s = T { field: vec![1], other: 2 };
    match &s {
        T { field, .. } => {
            s.other = 3;
            println!("{:?}", field);
        }
    }
    println!("{}", s.other);
}
