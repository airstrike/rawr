// rawr
mod receipt;
mod tax;

use crate::receipt::line;

fn main() {
    let prices = vec![1250, 399, 2000];
    let mut total = 0;
    for price in prices {
        println!("{}", line("item", price));
        total += price;
    }
    println!("{}", line("total", tax::with_tax(total)));
}
