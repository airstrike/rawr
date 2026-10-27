fn main() {
    let r: Result<String, i32> = Ok(String::from("ab"));
    let s = r.unwrap();
    println!("{} {:?}", s, r);
}
