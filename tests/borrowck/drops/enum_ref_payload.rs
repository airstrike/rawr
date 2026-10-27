struct Noisy(&'static str);

impl Drop for Noisy {
    fn drop(&mut self) {
        println!("drop {}", self.0);
    }
}

enum Slot<'a> {
    Lent(&'a Noisy),
    Owned(Noisy),
}

fn main() {
    let a = Noisy("a");
    let slots = vec![Slot::Lent(&a), Slot::Owned(Noisy("b")), Slot::Lent(&a)];
    println!("{}", slots.len());
    drop(slots);
    println!("end");
}
