//! The generated `ICONS` constant.
mod names;

pub fn source(list: &str) -> String {
    format!("pub const ICONS: &[&str] = &[{}];\n", names::quoted(list))
}
