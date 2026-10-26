fn main() {
    let mut o = Some(vec![1]);
    match o {
        Some(ref mut x) => {
            x.push(2);
            let n = o.is_some();
            println!("{}", n);
        }
        None => {}
    }
    println!("{:?}", o);
}
