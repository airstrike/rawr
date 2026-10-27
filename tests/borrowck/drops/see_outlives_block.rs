struct See<'a>(&'a i32);

impl Drop for See<'_> {
    fn drop(&mut self) {
        println!("see {}", self.0);
    }
}

fn main() {
    let y = 0;
    let mut s = See(&y);
    {
        let x = 1;
        s = See(&x);
    }
}
