use bank::Transaction;

fn category(tx: Transaction) -> Option<String> {
    if tx.payye.contains("Coffee") {
        Some("coffee".into())
    } else {
        None
    }
}
