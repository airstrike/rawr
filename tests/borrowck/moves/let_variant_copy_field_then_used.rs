enum E {
    V { s: String, n: i32 },
}

fn take(e: E) -> i32 {
    match e {
        E::V { n, .. } => n,
    }
}

fn main() {
    let e = E::V { s: String::from("ab"), n: 2 };
    let E::V { n, .. } = e;
    println!("{} {}", n, take(e));
}
