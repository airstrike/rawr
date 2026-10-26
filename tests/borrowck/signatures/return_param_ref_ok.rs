struct P {
    name: String,
    v: Vec<i32>,
}

impl P {
    fn name(&self) -> &str {
        &self.name
    }

    fn first(&self) -> &i32 {
        &self.v[0]
    }
}

fn first(v: &Vec<i32>) -> &i32 {
    &v[0]
}

fn pick<'a>(x: &'a i32) -> &'a i32 {
    let y = x;
    y
}

fn trimmed(s: &String) -> &str {
    s.trim()
}

fn main() {
    let p = P { name: String::from("n"), v: vec![3] };
    let v = vec![1, 2];
    println!("{} {} {} {} {}", p.name(), p.first(), first(&v), pick(&v[1]), trimmed(&p.name));
}
