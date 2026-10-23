// rawr
#[rawr::preview(name = "three")]
pub fn label(count: i64) -> String {
    format!("{count} items")
}
