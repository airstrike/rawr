struct See<'a>(&'a i32);

impl Drop for See<'_> {
    fn drop(&mut self) {
        println!("see {}", self.0);
    }
}

fn main() {
    let x = 1;
    let mut s = See(&x);
    let y = 2;
    s = See(&x);
    println!("{} {}", x, y);
}
