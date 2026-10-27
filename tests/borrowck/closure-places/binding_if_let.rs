fn main() {
    let mut slot = Some(vec![1, 2]);
    if let Some(v) = &mut slot {
        let mut f = || v.push(3);
        f();
    }
    println!("{slot:?}");
}
