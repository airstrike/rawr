//! A crate's function-like macro called from a program's body (#1125), as
//! `rust_decimal_macros::dec!(1.5)` is: rustc expands the call in a small
//! crate, as it expands a crate derive (`super`), and the expansion comes
//! back as source text. Each is kept in the project's `target/rawr/expand/`
//! at a path named for the call's macro, delimiters, input and place, so a
//! call an edit changes is a call with no expansion yet, and one an edit
//! leaves keeps its own. Its first line names the artifact of the macro's
//! crate it was expanded against and the call, so a build that changes the
//! crate expands the call again in its place (`super::kept`, #2152).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// Where a call is, which decides what rustc parses its expansion as.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Place {
    /// One expression.
    Expression,
    /// The statements of the block it is in.
    Statements,
    /// A type.
    Type,
    /// A pattern.
    Pattern,
}

/// The delimiters a call's input is in.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Delimiter {
    /// `m!(..)`.
    Parenthesis,
    /// `m![..]`.
    Bracket,
    /// `m! {..}`.
    Brace,
}

impl Delimiter {
    fn open(self) -> char {
        match self {
            Self::Parenthesis => '(',
            Self::Bracket => '[',
            Self::Brace => '{',
        }
    }

    fn close(self) -> char {
        match self {
            Self::Parenthesis => ')',
            Self::Bracket => ']',
            Self::Brace => '}',
        }
    }
}

/// One call of a crate's function-like macro.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Call {
    /// The macro's path from its crate's root (`rust_decimal_macros::dec`).
    pub path: String,
    pub delimiter: Delimiter,
    /// The tokens inside the delimiters, as text.
    pub input: String,
    pub place: Place,
}

/// The first line of an expansion source, before the artifact and the
/// call.
const SOURCE_HEADER: &str = "// call ";

impl Call {
    /// The call as Rust writes it: `rust_decimal_macros::dec!(1.5)`.
    fn text(&self) -> String {
        format!(
            "{}!{}{}{}",
            self.path,
            self.delimiter.open(),
            self.input,
            self.delimiter.close()
        )
    }

    /// Where the program at `project` keeps this call's expansion.
    #[must_use]
    pub fn source_path(&self, project: &Path) -> PathBuf {
        let mut digest = Sha256::new();
        digest.update(b"rawr.import.expand.call/v1");
        // The place's byte is its order in `Place`.
        digest.update([self.place as u8]);
        digest.update(self.text().as_bytes());
        super::directory(project).join(format!("call-{}.rs", super::hex(&digest.finalize())))
    }

    /// The call's function in the small crate: the call is its one `let`'s
    /// initializer, type or pattern, or its body, as the driver reads it
    /// back. A call in statement position is one as rustc parses it: in
    /// braces, which keep the last expression it expands to as the body's
    /// value, or in parentheses or brackets before a `;`, which rustc puts
    /// after the last statement it expands to.
    #[cfg_attr(
        not(feature = "rustc-driver"),
        allow(dead_code, reason = "only the driver's build runs an expansion")
    )]
    fn function(&self) -> String {
        match self.place {
            Place::Expression => {
                format!(
                    "fn __rawr_expression() {{\n    let _ = {};\n}}\n",
                    self.text()
                )
            }
            Place::Type => format!("fn __rawr_type() {{\n    let _: {};\n}}\n", self.text()),
            Place::Pattern => format!("fn __rawr_pattern() {{\n    let {};\n}}\n", self.text()),
            Place::Statements => {
                let end = if self.delimiter == Delimiter::Brace {
                    ""
                } else {
                    ";"
                };
                format!("fn __rawr_statements() {{\n    {}{end}\n}}\n", self.text())
            }
        }
    }
}

/// The first line of the source of `call`'s expansion against the
/// artifact whose digest is `artifact`.
#[cfg_attr(
    not(feature = "rustc-driver"),
    allow(dead_code, reason = "only the driver's build runs an expansion")
)]
fn header(call: &Call, artifact: &str) -> String {
    let call = serde_json::to_string(call).unwrap_or_default();
    format!("{SOURCE_HEADER}{artifact} {call}\n")
}

/// The call a kept expansion source `text` is of, and the digest of the
/// artifact it was expanded against. `None` for a text that is no call's
/// expansion source.
#[cfg_attr(
    not(feature = "rustc-driver"),
    allow(dead_code, reason = "only the driver's build runs an expansion")
)]
pub(super) fn built(text: &str) -> Option<(Call, &str)> {
    let (line, _) = text.split_once('\n')?;
    let (artifact, call) = line.strip_prefix(SOURCE_HEADER)?.split_once(' ')?;
    Some((serde_json::from_str(call).ok()?, artifact))
}

/// What a kept expansion source `text` holds after its header: the
/// expression or the statements the call expanded to. `None` for a text
/// that is no call's expansion source.
#[must_use]
pub fn kept(text: &str) -> Option<&str> {
    let (_, expansion) = text.strip_prefix(SOURCE_HEADER)?.split_once('\n')?;
    Some(expansion)
}

/// Expand each of `calls` of the program at `project` against `basis` in
/// one driver run, keeping each expansion where the program's next load
/// reads it, under the artifact of its macro's crate.
#[cfg(feature = "rustc-driver")]
pub fn expand(
    calls: &[Call],
    project: &Path,
    basis: &super::Basis,
    edition: &str,
    native: &crate::process::Native,
    limits: &crate::limits::Limits,
) -> crate::Result<()> {
    let directory = super::directory(project);
    let mut text = String::new();
    let mut modules = Vec::with_capacity(calls.len());
    for (index, call) in calls.iter().enumerate() {
        modules.push(u32::try_from(text.len()).unwrap_or(u32::MAX));
        text.push_str(&format!("mod __rawr_expand_{index} {{\n"));
        text.push_str(&call.function());
        text.push_str("}\n");
    }
    let roots = calls
        .iter()
        .map(|call| call.path.split("::").next().unwrap_or_default())
        .collect::<std::collections::BTreeSet<_>>();
    let expanded = super::drive(&text, &roots, basis, edition, &directory, native, limits)?;
    for (call, module) in calls.iter().zip(modules) {
        let called = expanded
            .calls
            .iter()
            .find(|called| called.at == module)
            .ok_or_else(|| crate::Error::Request {
                reason: format!("the driver wrote no expansion of `{}`", call.text()).into(),
            })?;
        let artifact = basis.artifact(&call.path).unwrap_or_default();
        let mut source = header(call, artifact);
        for item in &called.items {
            source.push_str(item);
            source.push('\n');
        }
        let path = call.source_path(project);
        let staged = path.with_extension(format!("rs.{}", std::process::id()));
        let io = |operation: &'static str| {
            let path = path.clone();
            move |source| crate::Error::Io {
                operation,
                path,
                source,
            }
        };
        std::fs::write(&staged, source).map_err(io("write a call's expansion"))?;
        std::fs::rename(&staged, &path).map_err(io("keep a call's expansion"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{built, header, kept, Call, Delimiter, Place};

    fn call(input: &str, place: Place) -> Call {
        Call {
            path: "numerals::double".to_owned(),
            delimiter: Delimiter::Parenthesis,
            input: input.to_owned(),
            place,
        }
    }

    #[test]
    fn a_calls_source_is_named_for_its_text_and_place() {
        let project = std::path::Path::new("/p");
        let path = call("3", Place::Expression).source_path(project);
        assert!(path.starts_with("/p/target/rawr/expand"));
        assert_eq!(path, call("3", Place::Expression).source_path(project));
        // An edit to the input, or the call's place, is another call.
        assert_ne!(path, call("4", Place::Expression).source_path(project));
        assert_ne!(path, call("3", Place::Statements).source_path(project));
        assert_eq!(
            call("3", Place::Expression).function(),
            "fn __rawr_expression() {\n    let _ = numerals::double!(3);\n}\n"
        );
        // Its source names the artifact it was expanded against and the
        // call, which a build of another artifact expands again.
        let source = format!(
            "{}((3) * 2)\n",
            header(&call("3 + 1", Place::Statements), "a1")
        );
        assert_eq!(kept(&source), Some("((3) * 2)\n"));
        assert_eq!(
            built(&source),
            Some((call("3 + 1", Place::Statements), "a1"))
        );
        assert_eq!(kept("((3) * 2)\n"), None);
    }
}
