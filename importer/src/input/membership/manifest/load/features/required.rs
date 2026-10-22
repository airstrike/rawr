//! Owned, nonterminal feature errors for Cargo callers that may discard them.
use super::*;
use crate::input::membership::manifest::load::normalize::target;

impl Features<'_, '_> {
    /// `version` is Cargo's already associated display string, not rustc's version.
    pub(in super::super::super) fn require_owned<A: document::Admission>(
        &self,
        feature: Feature,
        version: &str,
        session: &mut session::Session<'_, A>,
    ) -> Step<Result<(), String>> {
        if self.is_enabled(feature, session)? {
            return Ok(Ok(()));
        }
        // Names in the pinned table already use the output hyphen spelling.
        let name = target::formatted(format_args!("{}", feature.name()), session)?;
        session.string(version.len())?;
        let mut message = target::formatted(format_args!("feature `{name}` is required\n\nThe package requires the Cargo feature called `{name}`, but that feature is not stabilized in this version of Cargo ({version}).\n"), session)?;
        if self.nightly_features_allowed && self.is_local {
            let advice = target::formatted(format_args!("Consider adding `cargo-features = [\"{name}\"]` to the top of Cargo.toml (above the [package] table) to tell Cargo you are opting in to use this unstable feature.\n"), session)?;
            append(&mut message, &advice, session)?;
        } else {
            append(
                &mut message,
                if self.nightly_features_allowed {
                    "Consider trying a more recent nightly release.\n"
                } else {
                    "Consider trying a newer version of Cargo (this may require the nightly release).\n"
                },
                session,
            )?;
        }
        let docs = target::formatted(format_args!("See https://doc.rust-lang.org/nightly/cargo/{} for more information about the status of this feature.\n", feature.metadata().docs), session)?;
        append(&mut message, &docs, session)?;
        Ok(Err(message))
    }
}

pub(in super::super) fn append<A: document::Admission>(
    output: &mut String,
    text: &str,
    session: &mut session::Session<'_, A>,
) -> Step<()> {
    let Some(length) = output.len().checked_add(text.len()) else {
        return session.fail(Error::Overflow);
    };
    session.string(length)?;
    if length > output.capacity() {
        session.string(output.len())?;
        session.admit(document::Request::Allocation {
            bytes: length,
            alignment: 1,
        })?;
        if let Err(error) = output.try_reserve_exact(text.len()) {
            return session.fail(Error::Allocation(error));
        }
    }
    output.push_str(text);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::reference;

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
    fn owned_requirement_refusals_remain_terminal() {
        let parsed = document::parse("", 80, &mut reference::Account).unwrap();
        let mut reference_account = reference::Account;
        let mut setup = session::Session::new(&mut reference_account);
        let original =
            manifest::Original::interpret(value::Input::root(&parsed), &mut setup).unwrap();
        let features = Features::interpret(
            &original,
            Context {
                allow_features: None,
                nightly_features_allowed: true,
                channel: "nightly",
                is_local: true,
            },
            &mut Warnings::new(),
            &mut setup,
        )
        .unwrap();
        let mut complete = Account::default();
        let mut session = session::Session::new(&mut complete);
        assert!(features
            .require_owned(Feature::ProfileRustflags, "TEST CARGO", &mut session)
            .unwrap()
            .is_err());
        session.finish(Ok(())).unwrap();
        for refusal in 0..complete.calls {
            let mut account = Account {
                calls: 0,
                refuse: Some(refusal),
            };
            let mut session = session::Session::new(&mut account);
            assert!(matches!(
                features.require_owned(Feature::ProfileRustflags, "TEST CARGO", &mut session),
                Err(crate::input::membership::manifest::Stop::Refused)
            ));
            // Even the enabled path cannot revive an account after refusal.
            assert!(matches!(
                features.require_owned(Feature::Edition2024, "TEST CARGO", &mut session),
                Err(crate::input::membership::manifest::Stop::Refused)
            ));
        }
    }

    #[test]
    fn owned_requirement_can_be_discarded_without_poisoning_session() {
        for (nightly, local, advice) in [
            (true, true, "Consider adding `cargo-features = [\"profile-rustflags\"]` to the top of Cargo.toml (above the [package] table) to tell Cargo you are opting in to use this unstable feature.\n"),
            (true, false, "Consider trying a more recent nightly release.\n"),
            (false, true, "Consider trying a newer version of Cargo (this may require the nightly release).\n"),
        ] {
            let parsed = document::parse("", 80, &mut reference::Account).unwrap();
            let mut account = reference::Account;
            let mut session = session::Session::new(&mut account);
            let original = manifest::Original::interpret(value::Input::root(&parsed), &mut session).unwrap();
            let features = Features::interpret(&original, Context { allow_features: None, nightly_features_allowed: nightly, channel: "nightly", is_local: local }, &mut Warnings::new(), &mut session).unwrap();
            let error = features.require_owned(Feature::ProfileRustflags, "TEST CARGO", &mut session).unwrap().unwrap_err();
            assert_eq!(error, format!("feature `profile-rustflags` is required\n\nThe package requires the Cargo feature called `profile-rustflags`, but that feature is not stabilized in this version of Cargo (TEST CARGO).\n{advice}See https://doc.rust-lang.org/nightly/cargo/reference/unstable.html#profile-rustflags-option for more information about the status of this feature.\n"));
            drop(error);
            assert!(features.require_owned(Feature::Edition2024, "TEST CARGO", &mut session).unwrap().is_ok());
            session.finish(Ok(())).unwrap();
        }
    }
}
