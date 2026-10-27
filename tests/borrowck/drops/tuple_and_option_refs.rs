struct Noisy(&'static str);

impl Drop for Noisy {
    fn drop(&mut self) {
        println!("drop {}", self.0);
    }
}

fn main() {
    let a = Noisy("a");
    let t = (Noisy("t"), &a);
    let o = (Some(&a), Some(Noisy("o")));
    let r: Result<&Noisy, Noisy> = Ok(&a);
    println!("{} {}", (t.1).0, o.0.is_some());
    drop(r);
    println!("end");
}
