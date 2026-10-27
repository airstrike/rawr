struct T {
    field: Option<Vec<i64>>,
    other: i64,
}
fn main() {
    let mut s = T { field: Some(vec![1]), other: 2 };
    let r = &mut s;
    if let T { field: Some(v), .. } = r {
        r.other = 7;
        v.push(4);
        r.other += 1;
    }
    println!("{:?} {}", s.field, s.other);
}
