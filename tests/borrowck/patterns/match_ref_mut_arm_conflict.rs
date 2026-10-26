fn main() {
    let mut o = Some(vec![1]);
    match o {
        Some(ref mut x) => {
            let n = o.is_some();
            x.push(2);
            println!("{}", n);
        }
        None => {}
    }
    println!("{:?}", o);
}
