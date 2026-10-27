fn main() {
    let mut a = 1;
    let e = String::from("e");
    let r: Result<&i32, &String> = Ok(&a);
    let o = r.ok();
    a += 1;
    println!("{o:?}");
}
