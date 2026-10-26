struct Slot<'a> {
    value: Option<&'a i32>,
}

fn main() {
    let mut a = 1;
    let slot = Slot { value: Some(&a) };
    a += 1;
    println!("{:?}", slot.value);
}
