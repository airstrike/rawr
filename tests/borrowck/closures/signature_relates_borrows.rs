fn foo(_: impl FnOnce(&u32) -> &u32) {}
fn main() {
    let x = 22;
    foo(|a| &x);
}
