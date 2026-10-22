// rawr
use crate::tax::format_cents;

pub fn line(label: &str, cents: i64) -> String {
    format!("{label}: {}", format_cents(cents))
}
