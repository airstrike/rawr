struct Noisy(&'static str);

impl Drop for Noisy {
    fn drop(&mut self) {
        println!("drop {}", self.0);
    }
}

struct Holder<'a> {
    items: Vec<&'a Noisy>,
    own: Noisy,
}

impl Drop for Holder<'_> {
    fn drop(&mut self) {
        println!("holder of {}", self.items.len());
    }
}

fn main() {
    let a = Noisy("a");
    let b = Noisy("b");
    let h = Holder { items: vec![&a, &b], own: Noisy("own") };
    println!("made {}", h.items.len());
}
