//! Native Rust: no `// rawr` marker. Rawr builds this module with Cargo,
//! and the marked view calls into it.

/// The label the counter shows.
pub fn format_count(count: i64) -> String {
    let sign = if count < 0 { "minus " } else { "" };
    format!("Count: {sign}{}", count.abs())
}
