struct See<'a>(&'a i32);

impl Drop for See<'_> {
    fn drop(&mut self) {
        println!("see {}", self.0);
    }
}

fn main() {
    let mut x = 1;
    for i in 0..3 {
        let s = See(&x);
        if i == 1 {
            x += 1;
            continue;
        }
    }
}
