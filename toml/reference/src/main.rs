#![forbid(unsafe_code)]

use std::io::{self, Write};

mod fixtures;
mod json;
mod observe;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let mut selected = None;
    let mut list = false;
    match arguments.next().as_deref() {
        None => {}
        Some("--list") => list = true,
        Some("--case") => {
            selected = Some(arguments.next().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "--case requires a fixture id")
            })?);
        }
        Some("--help") => {
            println!("rawr-toml-reference [--list | --case ID]");
            return Ok(());
        }
        Some(_) => {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "unknown argument").into())
        }
    }
    if arguments.next().is_some() {
        return Err(
            io::Error::new(io::ErrorKind::InvalidInput, "unexpected extra argument").into(),
        );
    }

    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());
    let mut count = 0;
    let mut mismatches = 0;
    for case in fixtures::cases() {
        if selected.as_deref().is_some_and(|id| id != case.id) {
            continue;
        }
        count += 1;
        if list {
            output.write_all(b"{\"id\":")?;
            json::string(&mut output, case.id)?;
            output.write_all(b",\"local_test\":")?;
            json::string(&mut output, case.local_test)?;
            output.write_all(b",\"purpose\":")?;
            json::string(&mut output, case.purpose)?;
            write!(output, ",\"expected_acceptance\":{}}}\n", case.accepted)?;
        } else if !observe::case(&mut output, &case)? {
            mismatches += 1;
        }
    }
    output.flush()?;
    if count == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "unknown fixture id").into());
    }
    if mismatches != 0 {
        return Err(io::Error::other(format!(
            "{mismatches} fixture acceptance expectations differ from upstream; inspect emitted evidence"
        )).into());
    }
    Ok(())
}
