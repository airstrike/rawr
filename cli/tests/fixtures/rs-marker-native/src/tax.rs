//! Native Rust: no `// rawr` marker. Rawr builds this module with Cargo,
//! and the marked modules call into it.

/// The sales tax rate, in basis points.
const RATE: i64 = 825;

/// `cents` with sales tax added, rounded to the nearest cent.
pub fn with_tax(cents: i64) -> i64 {
    cents + (cents * RATE + 5_000) / 10_000
}

/// `cents` as dollars and cents.
pub fn format_cents(cents: i64) -> String {
    format!("${}.{:02}", cents / 100, cents % 100)
}
