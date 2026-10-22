// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Pinned edition parsing/MSRV/default warning, before unstable-edition gating.

use super::super::super::{scalar::Rust, session::Session, Step};
use super::super::{normalize::target, Warnings};
use rawr_toml::document;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Value {
    E2015,
    E2018,
    E2021,
    E2024,
    Future,
}
/// A selected edition only; unstable-feature authorization remains downstream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super) struct Edition(Value);
impl Edition {
    pub(in super::super) fn text(self) -> &'static str {
        match self.0 {
            Value::E2015 => "2015",
            Value::E2018 => "2018",
            Value::E2021 => "2021",
            Value::E2024 => "2024",
            Value::Future => "future",
        }
    }
    pub(in super::super) fn is_stable(self) -> bool {
        self.0 != Value::Future
    }
    pub(in super::super) fn at_least_2024(self) -> bool {
        matches!(self.0, Value::E2024 | Value::Future)
    }
    fn first_minor(self) -> Option<u64> {
        match self.0 {
            Value::E2015 | Value::Future => None,
            Value::E2018 => Some(31),
            Value::E2021 => Some(56),
            Value::E2024 => Some(85),
        }
    }
}

#[derive(Debug)]
pub(in super::super) enum Failure {
    Parse { cause: String },
    Incompatible(String),
}
impl Failure {
    pub(in super::super) fn context(&self) -> Option<&'static str> {
        match self {
            Self::Parse { .. } => Some("failed to parse the `edition` key"),
            Self::Incompatible(_) => None,
        }
    }
    pub(in super::super) fn message(&self) -> &str {
        match self {
            Self::Parse { cause } => cause,
            Self::Incompatible(message) => message,
        }
    }
}

pub(in super::super) fn select<A: document::Admission>(
    edition: Option<&str>,
    rust_version: Option<&Rust>,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<Result<Edition, Failure>> {
    session.check()?;
    if let Some(text) = edition {
        session.string(text.len())?;
        session.items(1)?;
        let parsed = match text {
            "2015" => Value::E2015,
            "2018" => Value::E2018,
            "2021" => Value::E2021,
            "2024" => Value::E2024,
            "future" => Value::Future,
            other => {
                let cause = if other
                    .parse::<u16>()
                    .is_ok_and(|year| year > 2024 && year < 2050)
                {
                    target::formatted(format_args!("this version of Cargo is older than the `{other}` edition, and only supports `2015`, `2018`, `2021`, and `2024` editions."), session)?
                } else {
                    target::formatted(format_args!("supported edition values are `2015`, `2018`, `2021`, or `2024`, but `{other}` is unknown"), session)?
                };
                return Ok(Err(Failure::Parse { cause }));
            }
        };
        let edition = Edition(parsed);
        if let (Some(msrv), Some(minor)) = (rust_version, edition.first_minor()) {
            session.items(1)?;
            if !compatible(msrv, minor) {
                let message = target::formatted(format_args!("rust-version {msrv} is incompatible with the version (1.{minor}.0) required by the specified edition ({})", edition.text()), session)?;
                return Ok(Err(Failure::Incompatible(message)));
            }
        }
        return Ok(Ok(edition));
    }
    let mut msrv_edition = Edition(Value::E2015);
    if let Some(msrv) = rust_version {
        // Edition2015 and Future have no first stable version and do not enter
        // Cargo's compatibility candidates. ALL order places newest last.
        for value in [Value::E2018, Value::E2021, Value::E2024] {
            session.items(1)?;
            let edition = Edition(value);
            if edition
                .first_minor()
                .is_some_and(|minor| compatible(msrv, minor))
            {
                msrv_edition = edition;
            }
        }
    }
    if msrv_edition.0 != Value::E2015 || rust_version.is_none() {
        let tip = if msrv_edition.0 == Value::E2024 || rust_version.is_none() {
            target::formatted(format_args!(" while the latest is `2024`"), session)?
        } else {
            target::formatted(
                format_args!(
                    " while {} is compatible with `rust-version`",
                    msrv_edition.text()
                ),
                session,
            )?
        };
        warnings.push(
            &[
                "`package.edition` is unspecified, defaulting to `2015`",
                &tip,
            ],
            session,
        )?;
    }
    session.items(1)?;
    Ok(Ok(Edition(Value::E2015)))
}

/// Fixed first-edition requirements are ^1.minor.0. Cargo turns an omitted
/// rustc minor/patch into zero; caret's upper bound excludes every other major.
/// The patch floor is zero, so it cannot reject a validated u64 patch.
fn compatible(rust: &Rust, minimum_minor: u64) -> bool {
    rust.major() == 1 && rust.minor().unwrap_or_default() >= minimum_minor
}

#[cfg(test)]
mod tests;
