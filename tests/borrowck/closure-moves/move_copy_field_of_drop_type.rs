struct Noisy(&'static str);

impl Drop for Noisy {
    fn drop(&mut self) {
        println!("drop {}", self.0);
    }
}

fn main() {
    let n = Noisy("n");
    let f = move || println!("{}", n.0);
    f();
    println!("{}", n.0);
}
