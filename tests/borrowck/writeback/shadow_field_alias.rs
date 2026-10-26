struct S {
    cells: Vec<i64>,
}
fn main() {
    let mut s = S { cells: vec![0] };
    {
        let r = &mut s.cells;
        let mut s = S { cells: vec![5] };
        r.push(1);
        s.cells.push(6);
        println!("{:?}", s.cells);
    }
    println!("{:?}", s.cells);
}
