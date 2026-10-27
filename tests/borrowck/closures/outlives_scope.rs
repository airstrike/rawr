fn main() {
    let mut o = None;
    {
        let v = vec![1, 2];
        o = Some(|| v.len());
    }
    if let Some(c) = o {
        println!("{}", c());
    }
}
