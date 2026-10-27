struct See<'a>(&'a i32);

impl Drop for See<'_> {
    fn drop(&mut self) {
        println!("see {}", self.0);
    }
}

fn main() {
    let mut x = 1;
    let f = |y: i32| {
        let s = See(&y);
        y + 1
    };
    x = f(x);
    println!("{}", x);
}
