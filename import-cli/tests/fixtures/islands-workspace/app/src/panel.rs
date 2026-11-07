// rawr
pub fn title() -> String {
    format!("{} / {}", model::rule(), sibling::greeting())
}
