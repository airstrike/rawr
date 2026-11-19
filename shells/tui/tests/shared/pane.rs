//! One pane of a screen's text, so a test reads what that pane shows and
//! not the same words in another: each pane is a box whose top border
//! carries its title, and its text is the rows inside that border, cut at
//! the box's sides.

/// The text inside the pane titled `title` on `screen`, one row per line,
/// or `None` when the screen draws no such pane.
pub fn pane(screen: &str, title: &str) -> Option<String> {
    let rows: Vec<Vec<char>> = screen.lines().map(|row| row.chars().collect()).collect();
    let heading: Vec<char> = format!("┌ {title} ").chars().collect();
    let (top, left) = rows.iter().enumerate().find_map(|(y, row)| {
        row.windows(heading.len())
            .position(|cells| cells == heading.as_slice())
            .map(|x| (y, x))
    })?;
    let right = left + rows[top][left..].iter().position(|cell| *cell == '┐')?;
    let mut text = String::new();
    for row in &rows[top + 1..] {
        if row.get(left) != Some(&'│') {
            break;
        }
        let inside: String = row[left + 1..right.min(row.len())].iter().collect();
        text.push_str(inside.trim_end());
        text.push('\n');
    }
    Some(text)
}
