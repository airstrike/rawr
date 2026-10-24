use shop::{tax_rate, Order};

fn price(subtotal: f64, items: i64) -> f64 {
    let rate = tax_rate("NY");
    let shipping = if items > 3 { 0.0 } else { 4.99 };
    subtotal * (1.0 + rate) + shipping
}

fn receipt(subtotal: f64, items: i64) -> String {
    format!("{items} items, {:.2} total", price(subtotal, items))
}

fn label(order: Order) -> String {
    format!("{} x{}", order.customer, order.items)
}

fn bigger(mut order: Order) -> f64 {
    order.add(3);
    price(order.total(10.0), order.items)
}
