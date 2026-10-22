//! Cargo read_manifest's final ordinary-then-critical warning attachment.
use super::{normalize::validate::Errors, Warnings};
use crate::input::membership::manifest::{session::Session, storage, Step};
use rawr_toml::document;

pub(super) struct Entry {
    pub(super) message: String,
    pub(super) critical: bool,
}
pub(super) struct Attached(storage::Buffer<Entry>);
impl Attached {
    pub(super) fn attach<A: document::Admission>(
        warnings: Warnings,
        errors: Errors,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        let mut entries = storage::Buffer::new();
        for message in warnings.0.into_iter() {
            entries.push(
                Entry {
                    message,
                    critical: false,
                },
                session,
            )?;
        }
        for message in errors.into_warnings().0.into_iter() {
            entries.push(
                Entry {
                    message,
                    critical: true,
                },
                session,
            )?;
        }
        Ok(Self(entries))
    }
    pub(super) fn as_slice(&self) -> &[Entry] {
        self.0.as_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Account {
        count: usize,
        refusal: Option<usize>,
    }
    impl document::Admission for Account {
        type Error = ();
        fn admit(&mut self, _: document::Request) -> Result<(), ()> {
            let current = self.count;
            self.count += 1;
            if self.refusal == Some(current) {
                Err(())
            } else {
                Ok(())
            }
        }
    }
    fn inputs() -> (Warnings, Errors, Vec<*const u8>) {
        let mut account = crate::input::membership::manifest::reference::Account;
        let mut session = Session::new(&mut account);
        let mut warnings = Warnings::new();
        let mut errors = Errors::new();
        for message in ["first", "second", "third"] {
            warnings.push(&[message], &mut session).unwrap();
        }
        for message in ["critical first", "critical second"] {
            errors.push(&[message], &mut session).unwrap();
        }
        let pointers = warnings
            .as_slice()
            .iter()
            .chain(errors.as_slice())
            .map(|s| s.as_ptr())
            .collect();
        (warnings, errors, pointers)
    }
    #[test]
    fn attachment_moves_text_in_order_and_refuses_each_admission() {
        let (warnings, errors, pointers) = inputs();
        let mut account = Account {
            count: 0,
            refusal: None,
        };
        let mut session = Session::new(&mut account);
        let attached = Attached::attach(warnings, errors, &mut session).unwrap();
        assert_eq!(
            attached
                .as_slice()
                .iter()
                .map(|e| (e.message.as_str(), e.critical))
                .collect::<Vec<_>>(),
            [
                ("first", false),
                ("second", false),
                ("third", false),
                ("critical first", true),
                ("critical second", true),
            ]
        );
        assert_eq!(
            attached
                .as_slice()
                .iter()
                .map(|e| e.message.as_ptr())
                .collect::<Vec<_>>(),
            pointers
        );
        session.finish(Ok(())).unwrap();
        let admissions = account.count;
        assert!(admissions > 5);
        for refusal in 0..admissions {
            let (warnings, errors, _) = inputs();
            let mut account = Account {
                count: 0,
                refusal: Some(refusal),
            };
            let mut session = Session::new(&mut account);
            let result = Attached::attach(warnings, errors, &mut session);
            assert!(result.is_err());
            assert!(session.finish(result).is_err());
            assert_eq!(account.count, refusal + 1);
        }
    }
}
