// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../../path/LICENSE-MIT and ../../path/LICENSE-APACHE.
//! Root-only checks after root and override feature validation.
use super::*;
use crate::input::membership::manifest::{
    load::Warnings,
    schema::{primitive, profile::tree::Value},
    Error,
};

pub(in super::super) fn validate<A: document::Admission>(
    profile: &Profile<'_>,
    name: &str,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<Result<(), String>> {
    session.check()?;
    session.string(name.len())?;
    if let Some(dir) = text(profile, Field::DirName, session)? {
        return target::formatted(format_args!("dir-name=\"{dir}\" in profile `{name}` is not currently allowed, directory names are tied to the profile name for custom profiles"), session).map(Err);
    }
    if text(profile, Field::Inherits, session)? == Some("debug") {
        return target::formatted(
            format_args!(
                "profile.{name}.inherits=\"debug\" should be profile.{name}.inherits=\"dev\""
            ),
            session,
        )
        .map(Err);
    }
    match name {
        "doc" => warnings.push(&["profile `doc` is deprecated and has no effect"], session)?,
        "test" | "bench" => {
            session.items(profile.entries().len())?;
            if profile.get(Field::Panic).is_some() {
                warnings.push(
                    &["`panic` setting is ignored for `", name, "` profile"],
                    session,
                )?;
            }
        }
        _ => {}
    }
    if let Some(panic) = text(profile, Field::Panic, session)?
        && !matches!(panic, "unwind" | "abort" | "immediate-abort")
    {
        return target::formatted(format_args!("`panic` setting of `{panic}` is not a valid setting, must be `unwind`, `abort`, or `immediate-abort`."), session).map(Err);
    }
    session.items(profile.entries().len())?;
    match profile.get(Field::Lto) {
        Some(Value::Primitive(primitive::Value::StringOrBool(
            primitive::StringOrBool::String(arg),
        ))) => {
            let arg = arg.as_str();
            session.string(arg.len())?;
            if matches!(arg, "true" | "false") {
                return target::formatted(format_args!("`lto` setting of string `\"{arg}\"` for `{name}` profile is not a valid setting, must be a boolean (`true`/`false`) or a string (`\"thin\"`/`\"fat\"`/`\"off\"`) or omitted."), session).map(Err);
            }
        }
        None
        | Some(Value::Primitive(primitive::Value::StringOrBool(
            primitive::StringOrBool::Boolean(_),
        ))) => {}
        _ => return session.fail(Error::State),
    }
    Ok(Ok(()))
}

fn text<'a, A: document::Admission>(
    profile: &'a Profile<'_>,
    field: Field,
    session: &mut Session<'_, A>,
) -> Step<Option<&'a str>> {
    session.items(profile.entries().len())?;
    match profile.get(field) {
        None => Ok(None),
        Some(Value::Primitive(primitive::Value::String(text))) => {
            session.string(text.as_str().len())?;
            Ok(Some(text.as_str()))
        }
        _ => session.fail(Error::State),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference, schema::profile::Profiles, value};

    #[test]
    fn root_checks_preserve_warning_and_error_order() {
        for (name, fields, error, count) in [
            ("dev", "lto=false", None, 0),
            ("dev", "lto='thin'", None, 0),
            ("dev", "lto='custom'", None, 0),
            ("doc", "", None, 1),
            ("test", "panic='abort'", None, 1),
            ("bench", "panic='invalid'\nlto='true'", Some("`panic` setting of `invalid` is not a valid setting, must be `unwind`, `abort`, or `immediate-abort`."), 1),
            ("doc", "inherits='debug'\npanic='invalid'", Some("profile.doc.inherits=\"debug\" should be profile.doc.inherits=\"dev\""), 0),
            ("doc", "dir-name='x'\ninherits='debug'", Some("dir-name=\"x\" in profile `doc` is not currently allowed, directory names are tied to the profile name for custom profiles"), 0),
            ("dev", "lto='true'", Some("`lto` setting of string `\"true\"` for `dev` profile is not a valid setting, must be a boolean (`true`/`false`) or a string (`\"thin\"`/`\"fat\"`/`\"off\"`) or omitted."), 0),
        ] {
            let source = format!("[profile.{name}]\n{fields}");
            let document = document::parse(&source, 4096, &mut reference::Account).unwrap();
            let mut account = reference::Account;
            let mut session = Session::new(&mut account);
            let profiles = Profiles::interpret(value::Input::raw(document.table().get("profile").unwrap()), &mut session).unwrap();
            let (_, _, profile) = profiles.entries().next().unwrap();
            let mut warnings = Warnings::new();
            let result = validate(profile, name, &mut warnings, &mut session).unwrap();
            assert_eq!(result.err().as_deref(), error, "{source}");
            assert_eq!(warnings.as_slice().len(), count, "{source}");
        }
    }
}
