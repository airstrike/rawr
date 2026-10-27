fn main() {
    let mut slot = Some(vec![1]);
    if let Some(v) = &mut slot {
        let mut f = || v.push(2);
        v.push(9);
        f();
    }
    println!("{slot:?}");
}
