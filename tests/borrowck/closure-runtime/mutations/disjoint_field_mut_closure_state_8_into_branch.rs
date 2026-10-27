struct State {
    items: Vec<i32>,
    log: Vec<String>,
}

fn main() {
    let mut state = State { items: vec![3, 1, 2], log: Vec::new() };
    let __b = &mut state;
    state.items.iter().for_each(|x| state.log.push(format!("saw {x}")));
    if __c() {
        __touch(__b);
    }
    let mut note = |text: &str| state.log.push(text.to_string());
    note("done");
    state.items.sort();
    println!("{:?} {:?}", state.items, state.log);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
