use bank::Transaction;

fn category(tx: Transaction) -> Option<String> {
    let payee = tx.payee.to_lowercase();
    if payee.contains("coffee") {
        Some("coffee".into())
    } else if payee.contains("whole foods") || payee.contains("trader joe") {
        Some("groceries".into())
    } else if payee == "netflix" {
        Some("streaming".into())
    } else if payee == "shell" {
        Some("fuel".into())
    } else if tx.is_income() {
        Some("income".into())
    } else {
        None
    }
}
