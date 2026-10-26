struct S {
    name: String,
}

fn tail(v: &Vec<i32>) -> &[i32] {
    &v[1..]
}

fn head(s: &str) -> &str {
    &s[..2]
}

fn whole(s: &String) -> &str {
    &s[..]
}

fn bytes(s: &String) -> &[u8] {
    &s.as_bytes()[1..]
}

fn name(s: &S) -> &str {
    &s.name[1..]
}

fn main() {
    let v = vec![1, 2, 3];
    let s = String::from("abc");
    let n = S { name: String::from("xyz") };
    println!("{:?} {} {} {:?} {}", tail(&v), head("hello"), whole(&s), bytes(&s), name(&n));
}
