fn call<F: FnMut()>(mut f: F) {
    f();
}

fn main() {
    let s = String::from("ab");
    call(|| drop(s));
}
