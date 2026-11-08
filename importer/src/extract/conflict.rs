//! The report of one definition that two fragments extracted with
//! different shapes. rawr merged fragments it should have kept apart, or
//! compared a field that differs per unit, so the report names the
//! definition, the unit each copy came from and the fields that differ.

use std::fmt::{self, Debug, Display, Write as _};

use crate::api::{identity, model};

use super::protocol;

/// The unit whose fragment carried one copy of a definition.
pub(super) struct Source {
    crate_name: Box<str>,
    target_kind: Box<str>,
    unit: Box<str>,
    artifact: Box<str>,
}

/// A merged entry and the fragment that first carried it.
pub(super) struct Sourced<T> {
    pub(super) value: T,
    pub(super) fragment: usize,
}

/// How much of a differing shape the report prints.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Detail {
    /// Every field's name, and the values of the short ones.
    Short,
    /// Every differing value in full, under `RAWR_VERBOSE=1`.
    Full,
}

impl Source {
    pub(super) fn new(handshake: &protocol::Handshake) -> Self {
        Self {
            crate_name: handshake.crate_name.as_str().into(),
            target_kind: handshake.target_kind.as_str().into(),
            unit: handshake.unit_correlation.as_str().into(),
            artifact: handshake.artifact.path.as_str().into(),
        }
    }
}

impl Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` ({}, unit {}, artifact {})",
            self.crate_name, self.target_kind, self.unit, self.artifact
        )
    }
}

impl Detail {
    pub(super) fn from_env() -> Self {
        match std::env::var_os("RAWR_VERBOSE") {
            Some(value) if value == "1" => Self::Full,
            Some(_) | None => Self::Short,
        }
    }
}

/// Two extractions of `first`'s identity whose shapes differ by more than
/// the display path.
pub(super) fn definition(
    first: (&model::Definition, &Source),
    second: (&model::Definition, &Source),
    detail: Detail,
) -> String {
    let (first, first_source) = first;
    let (second, second_source) = second;
    let mut report = heading(first.id(), first, first_source, second_source);
    report.push_str("; they differ in ");
    let mut fields = Fields::new(&mut report, detail);
    fields.short("owner", first.owner(), second.owner());
    fields.short("visibility", first.visibility(), second.visibility());
    fields.long("kind", first.kind(), second.kind());
    fields.short("span", first.span(), second.span());
    fields.short("origin", first.origin(), second.origin());
    fields.finish();
    report
}

/// Two rejections of one identity whose reasons or spans differ.
pub(super) fn rejection<Reason: Debug + PartialEq>(
    id: &identity::Definition,
    first: (&Reason, &model::Span, &Source),
    second: (&Reason, &model::Span, &Source),
    detail: Detail,
) -> String {
    let (first_reason, first_span, first_source) = first;
    let (second_reason, second_span, second_source) = second;
    let mut report = format!(
        "{} {} was rejected twice: first by {first_source}, then by {second_source}; \
         they differ in ",
        id.def_path_hash(),
        unit(id.unit()),
    );
    let mut fields = Fields::new(&mut report, detail);
    fields.long("reason", first_reason, second_reason);
    fields.short("span", first_span, second_span);
    fields.finish();
    report
}

/// The rejection of an identity that another fragment extracted.
pub(super) fn mixed(definition: (&model::Definition, &Source), rejected: &Source) -> String {
    let (definition, source) = definition;
    format!(
        "`{}` ({} {}) was extracted by {source} but rejected by {rejected}",
        path(definition),
        definition.id().def_path_hash(),
        unit(definition.id().unit()),
    )
}

fn heading(
    id: &identity::Definition,
    first: &model::Definition,
    first_source: &Source,
    second_source: &Source,
) -> String {
    format!(
        "`{}` ({} {} in {}) was extracted twice with different shapes: \
         first by {first_source}, then by {second_source}",
        path(first),
        id.namespace().as_str(),
        id.def_path_hash(),
        unit(id.unit()),
    )
}

fn path(definition: &model::Definition) -> String {
    definition.display_path().collect::<Vec<_>>().join("::")
}

fn unit(unit: &identity::Unit) -> String {
    let features = unit.features().collect::<Vec<_>>().join(",");
    format!(
        "crate `{}` [{features}] {}",
        unit.crate_name(),
        unit.stable_crate_id()
    )
}

/// The differing fields, written one after another into a report.
struct Fields<'a> {
    report: &'a mut String,
    detail: Detail,
    written: usize,
}

impl<'a> Fields<'a> {
    fn new(report: &'a mut String, detail: Detail) -> Self {
        Self {
            report,
            detail,
            written: 0,
        }
    }

    /// A field whose values are short enough to print every time.
    fn short<Value: Debug + PartialEq>(&mut self, name: &str, first: &Value, second: &Value) {
        if first != second {
            self.separate();
            let _ = write!(self.report, "{name} ({first:?} vs {second:?})");
        }
    }

    /// A field whose values may be long: their variants unless the detail
    /// is full.
    fn long<Value: Debug + PartialEq>(&mut self, name: &str, first: &Value, second: &Value) {
        if first == second {
            return;
        }
        self.separate();
        match self.detail {
            Detail::Full => {
                let _ = write!(self.report, "{name} ({first:?} vs {second:?})");
            }
            Detail::Short => {
                let _ = write!(
                    self.report,
                    "{name} ({} vs {}; RAWR_VERBOSE=1 prints both)",
                    variant(first),
                    variant(second)
                );
            }
        }
    }

    fn separate(&mut self) {
        if self.written > 0 {
            self.report.push_str(", ");
        }
        self.written += 1;
    }

    fn finish(self) {
        if self.written == 0 {
            self.report.push_str("nothing but the display path");
        }
    }
}

/// The leading variant name of a value's `Debug` form.
fn variant<Value: Debug>(value: &Value) -> String {
    let full = format!("{value:?}");
    let end = full
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(full.len());
    full[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::{Detail, Source};
    use crate::api::{identity, model};

    fn source(crate_name: &str, unit: &str) -> Source {
        Source {
            crate_name: crate_name.into(),
            target_kind: "lib".into(),
            unit: unit.into(),
            artifact: format!("/target/{unit}.rmeta").into(),
        }
    }

    fn id() -> identity::Definition {
        let package = identity::Package::new(
            "path+file:///w/shared#0.1.0".into(),
            "shared".into(),
            "0.1.0".into(),
            "path+file:///w/shared".into(),
        );
        let unit = identity::Unit::new(
            identity::Authority::Cargo(package),
            "shared".into(),
            "lib".into(),
            "shared".into(),
            identity::Compilation::Target,
            Box::new(["std".into()]),
            "0123456789abcdef".into(),
        );
        identity::Definition::new(
            unit,
            "520984daf8f8841502668e10003b1297".into(),
            identity::Namespace::Type,
        )
    }

    fn definition(file: &str, visibility: model::Visibility) -> model::Definition {
        model::Definition::new(
            id(),
            Box::new(["shared".into(), "Thing".into()]),
            model::Owner::Free,
            visibility,
            model::Kind::Module,
            model::Span::new(file.into(), 0, 5, 1, 0, 1, 5),
            model::Origin::Source,
        )
    }

    #[test]
    fn conflicting_definition_names_its_path_units_and_fields() {
        let first = definition("src/lib.rs", model::Visibility::Public);
        let second = definition("/w/shared/src/lib.rs", model::Visibility::Public);
        let report = super::definition(
            (&first, &source("shared", "u1")),
            (&second, &source("app", "u2")),
            Detail::Short,
        );
        assert!(report.starts_with("`shared::Thing` (type 520984daf8f8841502668e10003b1297"));
        assert!(report.contains("crate `shared` [std] 0123456789abcdef"));
        assert!(report.contains("first by `shared` (lib, unit u1"));
        assert!(report.contains("then by `app` (lib, unit u2"));
        assert!(report.contains("they differ in span ("), "{report}");
        assert!(report.contains("\"src/lib.rs\""), "{report}");
        assert!(report.contains("\"/w/shared/src/lib.rs\""), "{report}");
        assert!(!report.contains("visibility"), "{report}");
        assert!(!report.contains("kind"), "{report}");
    }

    #[test]
    fn conflicting_kind_prints_variants_unless_verbose() {
        let first = definition("src/lib.rs", model::Visibility::Public);
        let mut second = definition("src/lib.rs", model::Visibility::Public);
        second = model::Definition::new(
            second.id().clone(),
            Box::new(["app".into(), "Thing".into()]),
            second.owner().clone(),
            second.visibility().clone(),
            model::Kind::Rejected {
                class: model::RejectedClass::Declaration,
                reason: crate::api::ledger::Reason::UnnameablePrivate(id()),
            },
            second.span().clone(),
            second.origin().clone(),
        );
        let short = super::definition(
            (&first, &source("shared", "u1")),
            (&second, &source("app", "u2")),
            Detail::Short,
        );
        assert!(
            short.contains("kind (Module vs Rejected; RAWR_VERBOSE=1 prints both)"),
            "{short}"
        );
        let full = super::definition(
            (&first, &source("shared", "u1")),
            (&second, &source("app", "u2")),
            Detail::Full,
        );
        assert!(full.contains("UnnameablePrivate"), "{full}");
    }
}
