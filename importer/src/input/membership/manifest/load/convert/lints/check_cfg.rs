//! String-list conversion after Cargo's check-cfg value clone; not final flags.
use crate::input::membership::manifest::{
    load::normalize::target, session::Session, storage, value, Step,
};
use rawr_toml::document;

pub(in super::super::super) fn strings<A: document::Admission>(
    input: value::Input<'_, '_>,
    session: &mut Session<'_, A>,
) -> Step<Result<storage::Buffer<String>, String>> {
    session.check()?;
    let mut strings = storage::Buffer::new();
    let value::View::Sequence(mut cursor) = input.view(session)? else {
        return invalid(session);
    };
    while let Some((_, child)) = cursor.next(session)? {
        session.items(1)?;
        let value::View::Scalar(value::Kind::String(text)) = child.view(session)? else {
            // Failed Vec<String> conversion drops its converted prefix before
            // Cargo allocates the outer diagnostic. Spent charges stay spent.
            drop(strings);
            return invalid(session);
        };
        session.string(text.as_str().len())?;
        let owned = target::formatted(format_args!("{}", text.as_str()), session)?;
        session.items(strings.len())?;
        strings.push(owned, session)?;
    }
    Ok(Ok(strings))
}

pub(super) fn invalid<T, A: document::Admission>(
    session: &mut Session<'_, A>,
) -> Step<Result<T, String>> {
    target::formatted(
        format_args!("`lints.rust.unexpected_cfgs.check-cfg` must be a list of string"),
        session,
    )
    .map(Err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference, Stop};
    #[derive(Default)]
    struct Account {
        calls: usize,
        refuse: Option<usize>,
    }
    impl document::Admission for Account {
        type Error = ();
        fn admit(&mut self, _: document::Request) -> Result<(), ()> {
            let index = self.calls;
            self.calls += 1;
            if self.refuse == Some(index) {
                Err(())
            } else {
                Ok(())
            }
        }
    }
    #[test]
    fn string_lists_preserve_duplicates_and_reject_other_types() {
        for (source, expected) in [
            ("v=[]", Some(vec![])),
            (
                "v=['cfg(a)','cfg(a)','']",
                Some(vec!["cfg(a)", "cfg(a)", ""]),
            ),
            ("v=['cfg(a)',42]", None),
            ("v='cfg(a)'", None),
            ("v={a='cfg(a)'}", None),
            ("v=[['cfg(a)']]", None),
        ] {
            let document = document::parse(source, 100, &mut reference::Account).unwrap();
            let input = value::Input::raw(document.table().get("v").unwrap());
            let mut account = Account::default();
            let mut session = Session::new(&mut account);
            let result = strings(input, &mut session).unwrap();
            match expected {
                Some(expected) => assert_eq!(result.unwrap().as_slice(), expected),
                None => assert_eq!(
                    result.err().unwrap(),
                    "`lints.rust.unexpected_cfgs.check-cfg` must be a list of string"
                ),
            }
            for refusal in 0..account.calls {
                let mut account = Account {
                    refuse: Some(refusal),
                    ..Account::default()
                };
                let mut session = Session::new(&mut account);
                assert!(matches!(strings(input, &mut session), Err(Stop::Refused)));
                assert!(matches!(strings(input, &mut session), Err(Stop::Refused)));
                assert_eq!(account.calls, refusal + 1);
            }
        }
    }
}
