struct See<'a>(&'a i32);

impl Drop for See<'_> {
    fn drop(&mut self) {
        println!("see {}", self.0);
    }
}

struct Pair<'a>(&'a Vec<i32>, i32);

fn main() {
    let mut v = vec![];
    let p = Pair(&v, 1);
    v.push(2);
    println!("{}", v.len());
}
