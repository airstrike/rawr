struct Noisy(&'static str);

impl Drop for Noisy {
    fn drop(&mut self) {
        println!("drop {}", self.0);
    }
}

struct Wrap<T>(T);

impl<T> Drop for Wrap<T> {
    fn drop(&mut self) {
        println!("drop wrap");
    }
}

fn main() {
    let inner = Wrap(Noisy("inner"));
    let outer = Wrap(&inner);
    let owned = Wrap(Noisy("owned"));
    println!("made");
    drop(outer);
    drop(owned);
    println!("end");
}
