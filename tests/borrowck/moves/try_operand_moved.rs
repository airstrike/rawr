fn f(r: Result<String, String>) -> Result<usize, String> {
    let s = r?;
    let t = r;
    Ok(s.len() + t.map(|t| t.len()).unwrap_or(0))
}

fn main() {
    println!("{:?}", f(Ok(String::from("ab"))));
}
