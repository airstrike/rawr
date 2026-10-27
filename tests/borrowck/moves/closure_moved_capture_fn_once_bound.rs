fn call<F: FnOnce()>(f: F) {
    f();
}

fn main() {
    let s = String::from("ab");
    call(|| drop(s));
    println!("done");
}
