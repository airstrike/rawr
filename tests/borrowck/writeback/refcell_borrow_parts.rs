use std::cell::RefCell;

fn main() {
    let c = RefCell::new(Some(vec![1]));
    if let Some(v) = &mut *c.borrow_mut() {
        v.push(2);
        v.push(3);
    }
    println!("{:?}", c.borrow());
    let pair = RefCell::new((0, vec![5]));
    match &mut *pair.borrow_mut() {
        (n, list) => {
            *n += list.len() as i64;
            list.push(6);
        }
    }
    println!("{:?}", pair.borrow());
}
