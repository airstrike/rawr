fn double(x: i32) -> i32 {
    x * 2
}

fn negate(x: i32) -> i32 {
    -x
}

struct Panel {
    styles: [(&'static str, fn(i32) -> i32); 2],
}

impl Panel {
    fn render(&self) -> Vec<i32> {
        let first = self.styles;
        let mut out: Vec<i32> = first.into_iter().map(|(_, f)| f(1)).collect();
        out.extend(self.styles.into_iter().map(|(_, f)| f(2)));
        out
    }
}

fn main() {
    let panel = Panel { styles: [("double", double), ("negate", negate)] };
    for __i in 0..2 {
        if __c() {
            __take(panel);
        }
    }
    println!("{:?} {}", panel.render(), panel.styles[0].0);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
