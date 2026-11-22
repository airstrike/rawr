//! A place in rawr's copy of the program's crate, as the program's own.
//!
//! The importer reads the program's native modules from the copy rawr
//! builds them from (`<target>/rawr/crate/`), so the places it records are
//! the copy's. The copy keeps each native module's lines, and only adds
//! `pub ` before an item native code must reach, so a place maps to the
//! same line of the program's file, after what the copy added before it.

use std::path::{Path, PathBuf};

use crate::diagnostic;

/// `span`'s place in the program's own file when `span` is in rawr's copy
/// of the program's crate under `project`: `None` for a span elsewhere, or
/// one whose file or line the program no longer has.
pub(super) fn program(
    span: &rawr_import::api::model::Span,
    project: &Path,
) -> Option<Result<diagnostic::ExternalPosition, diagnostic::Error>> {
    let file = program_file(span.file(), project)?;
    let copy = std::fs::read_to_string(span.file()).ok()?;
    let text = std::fs::read_to_string(&file).ok()?;
    let bytes = span.byte_range();
    let start = Place::of(&copy, bytes.start as usize)?.in_program(&text)?;
    let end = Place::of(&copy, bytes.end as usize)?.in_program(&text)?;
    Some(diagnostic::ExternalPosition::checked(
        file.to_string_lossy().into_owned(),
        start.byte,
        end.byte,
        (start.line, start.column),
        (end.line, end.column),
    ))
}

/// The program's file that `copied`, a file of rawr's copy, copies: the
/// copy's path below the workspace, as Cargo names it, under the nearest
/// of `project`'s directories that has it.
fn program_file(copied: &str, project: &Path) -> Option<PathBuf> {
    let relative = rawr_import::location::copy::program(copied);
    if relative == copied {
        return None;
    }
    project
        .ancestors()
        .map(|directory| directory.join(&relative))
        .find(|candidate| candidate.is_file())
}

/// A byte of the copy: its line, from 1, and the rest of its line after it.
struct Place<'copy> {
    line: usize,
    rest: &'copy str,
}

/// A place in the program's file: its byte, its line from 1 and its column
/// in characters from 0, as the importer's spans count them.
struct Mapped {
    byte: u32,
    line: u32,
    column: u32,
}

impl<'copy> Place<'copy> {
    fn of(copy: &'copy str, byte: usize) -> Option<Self> {
        let before = copy.get(..byte)?;
        let line_end = copy[byte..].find('\n').map_or(copy.len(), |at| byte + at);
        Some(Self {
            line: before.matches('\n').count() + 1,
            rest: copy[byte..line_end].trim_end_matches('\r'),
        })
    }

    /// The same place in `text`, the program's file: on the same line,
    /// before the same rest of the line, or before what follows the `pub `
    /// the copy added there.
    fn in_program(&self, text: &str) -> Option<Mapped> {
        let line_start = text
            .split_inclusive('\n')
            .take(self.line - 1)
            .map(str::len)
            .sum::<usize>();
        let written = text[line_start..]
            .split('\n')
            .next()?
            .trim_end_matches('\r');
        let rest = if written.ends_with(self.rest) {
            self.rest
        } else {
            self.rest
                .strip_prefix("pub ")
                .filter(|rest| written.ends_with(rest))?
        };
        let at = written.len() - rest.len();
        Some(Mapped {
            byte: u32::try_from(line_start + at).ok()?,
            line: u32::try_from(self.line).ok()?,
            column: u32::try_from(written[..at].chars().count()).ok()?,
        })
    }
}

#[cfg(test)]
mod tests {
    /// A native module's item, which rawr's copy made public, is at its
    /// own line and column of the program's file.
    #[test]
    fn a_place_in_the_copy_is_the_program_s() {
        let copy = "use std::fmt;\npub fn describe() -> String {\n    String::new()\n}\n";
        let program = "use std::fmt;\nfn describe() -> String {\n    String::new()\n}\n";
        let name = copy.find("describe").unwrap();
        let mapped = super::Place::of(copy, name)
            .unwrap()
            .in_program(program)
            .unwrap();
        assert_eq!(mapped.byte as usize, program.find("describe").unwrap());
        assert_eq!((mapped.line, mapped.column), (2, 3));
        let item = copy.find("pub fn").unwrap();
        let mapped = super::Place::of(copy, item)
            .unwrap()
            .in_program(program)
            .unwrap();
        assert_eq!(mapped.byte as usize, program.find("fn describe").unwrap());
        let changed = "use std::fmt;\nfn other() {}\n";
        assert!(super::Place::of(copy, name)
            .unwrap()
            .in_program(changed)
            .is_none());
    }
}
