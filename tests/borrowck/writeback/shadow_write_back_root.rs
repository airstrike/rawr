fn main() {
    let mut v = Some(vec![0]);
    if let Some(x) = &mut v {
        let mut v = Some(vec![9]);
        x.push(1);
        if let Some(y) = &mut v {
            y.push(8);
        }
        println!("{v:?}");
    }
    println!("{v:?}");
}
