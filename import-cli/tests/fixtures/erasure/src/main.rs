// rawr
// The erasure checks (review 1, 2, 3 and 9): each case's test writes its
// own `main` in place of this one.
fn main() {
    let resource = natives::identity(natives::Resource::new(1));
    println!("id {}", resource.id());
}
