struct See<'a>(&'a i32);

impl Drop for See<'_> {
    fn drop(&mut self) {
        println!("see {}", self.0);
    }
}

fn main() {
    let mut x = 1;
    match x {
        1 => {
            let s = See(&x);
            x = 2;
        }
        _ => {}
    }
}
