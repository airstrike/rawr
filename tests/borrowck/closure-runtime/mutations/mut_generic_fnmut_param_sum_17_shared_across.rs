fn apply<F: FnMut(i32) -> i32>(f: &mut F, values: &[i32]) -> i32 {
    let mut last = 0;
    for value in values {
        last = f(*value);
    }
    last
}

fn main() {
    let mut sum = 0;
    let mut running = |x| {
        sum += x;
        sum
    };
    println!("{}", apply(&mut running, &[1, 2, 3]));
    println!("{}", apply(&mut running, &[4]));
    let __s = &sum;
    println!("{sum}");
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
