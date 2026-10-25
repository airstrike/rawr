use bank::Transaction;

fn category(tx: Transaction) -> String {
    if tx.payee.contains("Coffee") {
        "coffee".into()
    } else {
        "other".into()
    }
}
