//! A failed runner build's error in an adapter rawr generated, as the
//! program's call that needed the adapter.
//!
//! The generated manifest's source map records, for each adapter's byte
//! range in its generated file, the program's use site that demanded it.
//! rustc's place in an adapter maps through it to that use site, and the
//! generated place stays in a note, as rustc names the code a macro
//! expanded to.

use std::ops::Range;

/// One record of the source map: an adapter's range in its generated file
/// and the program's use site that demanded it.
#[derive(serde::Deserialize)]
struct Record {
    file: Box<str>,
    generated: Range<u32>,
    rawr: crate::surface::Origin,
}

/// `error` with each place rustc's errors name in an adapter as the
/// program's call that demanded the adapter, read through `source_map` and
/// `files`, the runner's generated files: `error` itself when it is no
/// Cargo failure or names no adapter.
pub(super) fn mapped(
    error: crate::Error,
    source_map: &[u8],
    files: &[crate::runner::manifest::File],
) -> crate::Error {
    let crate::Error::Process {
        program: "cargo",
        status,
        stderr,
    } = error
    else {
        return error;
    };
    let records: Vec<Record> = serde_json::from_slice(source_map).unwrap_or_default();
    let stderr = rewritten(&stderr, &records, |path| {
        files
            .iter()
            .find(|file| file.path() == path)
            .map(crate::runner::manifest::File::bytes)
    });
    crate::Error::Process {
        program: "cargo",
        status,
        stderr: stderr.into_boxed_str(),
    }
}

/// `stderr` with each `-->` line in an adapter as the program's use site,
/// followed by the note naming the generated place.
fn rewritten<'f>(
    stderr: &str,
    records: &[Record],
    generated: impl Fn(&str) -> Option<&'f [u8]>,
) -> String {
    let mut written = String::with_capacity(stderr.len());
    for line in stderr.split_inclusive('\n') {
        let Some((indent, place)) = line
            .trim_end()
            .split_once("--> ")
            .filter(|(indent, _)| indent.trim().is_empty())
        else {
            written.push_str(line);
            continue;
        };
        match call_site(place.trim(), records, &generated) {
            Some(site) => {
                use std::fmt::Write as _;
                let _ = writeln!(
                    written,
                    "{indent}--> {site}\n{indent} = note: {}{place}",
                    crate::location::copy::CALL_SITE
                );
            }
            None => written.push_str(line),
        }
    }
    written
}

/// The program's use site, `file:line:column`, of the adapter `place`, a
/// generated `file:line:column`, is in.
fn call_site<'f>(
    place: &str,
    records: &[Record],
    generated: &impl Fn(&str) -> Option<&'f [u8]>,
) -> Option<String> {
    let mut parts = place.rsplitn(3, ':');
    let column = parts.next()?.parse::<usize>().ok()?;
    let line = parts.next()?.parse::<usize>().ok()?;
    let path = parts.next()?;
    let record_of = |file: &str| path == file || path.ends_with(&format!("/{file}"));
    let file = records.iter().find(|record| record_of(&record.file))?;
    let text = std::str::from_utf8(generated(&file.file)?).ok()?;
    let offset = u32::try_from(byte_of(text, line, column)?).ok()?;
    let record = records
        .iter()
        .find(|record| record.file == file.file && record.generated.contains(&offset))?;
    let origin = &record.rawr;
    let source = std::fs::read_to_string(origin.source()).ok()?;
    let start = usize::try_from(origin.byte_range().start).ok()?;
    let before = source.get(..start)?;
    let line = before.matches('\n').count() + 1;
    let column = before[before.rfind('\n').map_or(0, |at| at + 1)..]
        .chars()
        .count()
        + 1;
    Some(format!("{}:{line}:{column}", origin.source()))
}

/// The byte offset of `line` and `column`, from 1, in `text`, the column
/// counted in characters as rustc counts it.
fn byte_of(text: &str, line: usize, column: usize) -> Option<usize> {
    let start: usize = text
        .split_inclusive('\n')
        .take(line.checked_sub(1)?)
        .map(str::len)
        .sum();
    let written = text.get(start..)?.split('\n').next()?;
    let at = written
        .char_indices()
        .nth(column.checked_sub(1)?)
        .map_or(written.len(), |(at, _)| at);
    Some(start + at)
}

#[cfg(test)]
mod tests {
    /// rustc's error in an adapter names the program's call that needed
    /// it, and the generated place in a note.
    #[test]
    fn an_error_in_an_adapter_names_the_program_s_call() {
        let root =
            std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/tests"))
                .join(format!("rawr-call-site-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let program = root.join("home.rs");
        let text = "// rawr\nfn view() {\n    row().spacing(\"zero\");\n}\n";
        std::fs::write(&program, text).unwrap();
        let start = u32::try_from(text.find(".spacing").unwrap() + 1).unwrap();
        let generated = "use super::*;\n\npub fn adapter() {\n    let output = iced::widget::Row::spacing(row, \"zero\");\n}\n";
        let body = u32::try_from(generated.find("pub fn").unwrap()).unwrap();
        let records = vec![super::Record {
            file: "src/__rawr_program/rawr_adapters_iced_8.rs".into(),
            generated: body..u32::try_from(generated.len()).unwrap(),
            rawr: crate::surface::Origin::checked(&program.to_string_lossy(), start, start + 7)
                .unwrap(),
        }];
        let stderr = "error[E0277]: the trait bound `Pixels: From<&str>` is not satisfied\n   \
             --> src/__rawr_program/rawr_adapters_iced_8.rs:4:50\n    |\n";
        let shown = super::rewritten(stderr, &records, |path| {
            (path == "src/__rawr_program/rawr_adapters_iced_8.rs").then_some(generated.as_bytes())
        });
        assert_eq!(
            shown,
            format!(
                "error[E0277]: the trait bound `Pixels: From<&str>` is not satisfied\n   \
                 --> {}:3:11\n    = note: in the code rawr generated for this call, at \
                 src/__rawr_program/rawr_adapters_iced_8.rs:4:50\n    |\n",
                program.display()
            )
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
