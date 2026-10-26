fn set(a: &mut i64) -> i64 {
    *a = 7;
    1
}

fn main() {
    let mut p = (1, 2);
    match &mut p {
        (a, _) => {
            p.0 = set(a);
        }
    }
    println!("{:?}", p);
}
