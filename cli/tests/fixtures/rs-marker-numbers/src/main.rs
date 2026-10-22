// rawr
// Every Rust number type crosses between Rawr and native Rust.
mod bits;

fn main() {
    let byte: u8 = bits::low_byte(0x1234);
    println!("low byte: {}", byte);
    println!("widened: {}", bits::widen(byte, 300));
    println!("half: {}", bits::half(5.0));
    println!("negated: {}", bits::negate(-100));
    println!("before: {}", bits::before(0));
    println!("fits: {:?} {:?}", bits::to_byte(255), bits::to_byte(256));
    println!("sum: {}", bits::sum(1, 2, 3, 4));
}
