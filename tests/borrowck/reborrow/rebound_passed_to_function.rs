fn add(v: &mut Vec<i32>, n: i32) {
    v.push(n);
}

fn total(v: &Vec<i32>) -> i32 {
    v.iter().sum()
}

fn main() {
    let mut a = vec![1];
    let mut b = vec![2];
    let mut r = &mut a;
    for n in 0..4 {
        add(r, n);
        println!("{}", total(r));
        if n == 1 {
            r = &mut b;
        }
    }
    println!("{a:?} {b:?}");
}
