struct See<'a>(&'a i32);

impl Drop for See<'_> {
    fn drop(&mut self) {
        println!("see {}", self.0);
    }
}

fn make(x: &i32) -> See<'_> {
    See(x)
}

fn main() {
    let mut x = 1;
    let s = make(&x);
    x = 2;
}
