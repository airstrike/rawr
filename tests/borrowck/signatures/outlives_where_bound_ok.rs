fn second<'a, 'b>(_x: &'a i32, y: &'b i32) -> &'a i32
where
    'b: 'a,
{
    y
}

fn main() {
    println!("{}", second(&1, &2));
}
