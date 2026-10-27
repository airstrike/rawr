struct See<'a>(&'a i32);

impl Drop for See<'_> {
    fn drop(&mut self) {
        println!("see {}", self.0);
    }
}

fn main() {
    let mut x = 1;
    let _ = See(&x);
    x = 2;
    println!("{}", x);
}
