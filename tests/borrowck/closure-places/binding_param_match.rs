fn grow(slot: &mut Option<Vec<i32>>) {
    if let Some(v) = slot {
        let mut push = |x| v.push(x);
        push(1);
        push(2);
    }
}

fn main() {
    let mut slot = Some(vec![0]);
    grow(&mut slot);
    println!("{slot:?}");
}
