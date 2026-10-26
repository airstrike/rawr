fn main() {
    let mut b = vec![0];
    let r = &mut b;
    let pick = Some(vec![3]);
    match pick {
        Some(mut b) => {
            r.push(1);
            b.push(4);
            println!("{b:?}");
        }
        None => {}
    }
    r.push(2);
    println!("{r:?}");
}
