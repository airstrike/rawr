struct See<'a>(&'a i32);

impl Drop for See<'_> {
    fn drop(&mut self) {
        println!("see {}", self.0);
    }
}

impl See<'_> {
    fn get(&self) -> i32 {
        *self.0
    }
}

fn bump(x: &mut i32) -> i32 {
    *x += 1;
    *x
}

fn main() {
    let mut x = 1;
    let n = See(&x).get() + bump(&mut x);
    println!("{}", n);
}
