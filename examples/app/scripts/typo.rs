use shop::Order;

fn label(order: Order) -> String {
    format!("{} x{}", order.customer, order.itmes)
}
