struct Noisy(&'static str);

impl Drop for Noisy {
    fn drop(&mut self) {
        println!("drop {}", self.0);
    }
}

struct Lock<'a>(&'a Noisy);

impl Drop for Lock<'_> {
    fn drop(&mut self) {
        println!("unlock {}", (self.0).0);
    }
}

fn main() {
    let owner = Noisy("owner");
    {
        let _lock = Lock(&owner);
        println!("locked");
    }
    println!("end");
}
