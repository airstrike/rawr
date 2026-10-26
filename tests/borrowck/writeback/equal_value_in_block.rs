fn main() {
    let mut q = (1, 2);
    match &mut q {
        (a, _) => {
            q.0 = {
                *a = 7;
                1
            };
        }
    }
    println!("{:?}", q);
}
