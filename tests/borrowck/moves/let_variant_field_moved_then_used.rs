enum E {
    V { s: String, n: i32 },
}

fn take(_: E) {}

fn main() {
    let e = E::V { s: String::from("ab"), n: 2 };
    let E::V { s, n } = e;
    println!("{} {}", s, n);
    take(e);
}
