struct See<'a>(&'a i32);

impl Drop for See<'_> {
    fn drop(&mut self) {
        println!("see {}", self.0);
    }
}

fn main() {
    let mut x = 1;
    let n = {
        let s = See(&x);
        *s.0 + 1
    };
    x = n;
    println!("{}", x);
}
