// rawr
//! A small grep: prints the lines of its files, or of stdin, that contain
//! a pattern.
//!
//!     rawr run examples/grep -- [-i] [-n] [-c] [-v] PATTERN [FILE...]
//!
//! `cargo build --release` builds the same crate natively.

mod search;

use search::Search;

const USAGE: &str = "usage: grep [-i] [-n] [-c] [-v] PATTERN [FILE...]";

fn main() -> Result<(), String> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let mut search = Search::new();
    let mut pattern = None;
    let mut files = Vec::new();
    for argument in arguments {
        match argument.as_str() {
            "-i" => search.ignore_case = true,
            "-n" => search.line_numbers = true,
            "-c" => search.count = true,
            "-v" => search.invert = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            _ if pattern.is_none() => pattern = Some(argument),
            _ => files.push(argument),
        }
    }
    let Some(pattern) = pattern else {
        return Err(USAGE.to_string());
    };
    search.pattern = pattern;

    // No files: each line of stdin as it comes.
    if files.is_empty() {
        let stdin = std::io::stdin();
        let mut line = String::new();
        let mut number = 0;
        let mut count = 0;
        loop {
            line.clear();
            let read = stdin
                .read_line(&mut line)
                .map_err(|error| format!("stdin: {error}"))?;
            if read == 0 {
                break;
            }
            number += 1;
            let text = line.trim_end_matches('\n');
            if search.matches(text) {
                count += 1;
                if !search.count {
                    println!("{}", search.show(None, number, text));
                }
            }
        }
        if search.count {
            println!("{}", search.total(None, count));
        }
        return Ok(());
    }

    let named = files.len() > 1;
    for file in &files {
        let text = std::fs::read_to_string(file).map_err(|error| format!("{file}: {error}"))?;
        let name = if named { Some(file.as_str()) } else { None };
        let mut count = 0;
        for (index, line) in text.lines().enumerate() {
            if search.matches(line) {
                count += 1;
                if !search.count {
                    println!("{}", search.show(name, index + 1, line));
                }
            }
        }
        if search.count {
            println!("{}", search.total(name, count));
        }
    }
    Ok(())
}
