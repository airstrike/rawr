use super::*;

#[derive(Default)]
struct Account {
    requests: Vec<document::Request>,
    refusal: Option<usize>,
    caps: Option<[usize; 3]>,
    used: [usize; 3],
}
impl document::Admission for Account {
    type Error = &'static str;
    fn admit(&mut self, request: document::Request) -> Result<(), Self::Error> {
        let index = self.requests.len();
        self.requests.push(request);
        match request {
            document::Request::Items(count) => self.used[0] += count,
            // Request::String is per-string, not a cumulative byte charge.
            document::Request::String(length) => self.used[1] = self.used[1].max(length),
            document::Request::Allocation { bytes, .. } => self.used[2] += bytes,
        }
        if self.refusal == Some(index)
            || self
                .caps
                .is_some_and(|caps| self.used.iter().zip(caps).any(|(used, cap)| *used > cap))
        {
            Err("inheritance admission")
        } else {
            Ok(())
        }
    }
}

const SOURCE: &str = "member={workspace=true,default-features=false,features=['member']}\n[workspace.dependencies]\ndep={path='dependency/../dep',features=['workspace']}";

// The same session first interprets the declarations, then accounts for the
// stage. Its measured prefix is never reset when inheritance starts.
fn run(account: &mut Account) -> Result<(), Error<&'static str>> {
    let document = document::parse(SOURCE, 80, &mut reference::Account).unwrap();
    let mut session = Session::new(account);
    let result = (|| {
        let member = Inheritable::interpret(
            value::Input::raw(document.table().get("member").unwrap()),
            &mut session,
        )?;
        let declaration = workspace::Workspace::interpret(
            value::Input::raw(document.table().get("workspace").unwrap()),
            &mut session,
        )?;
        let progress = dependency("dep", &member, Path::new("/ws/member"), 2021, &mut session)?;
        let Progress::Workspace(request) = progress else {
            panic!("workspace")
        };
        let mut warnings = Warnings::new();
        let view = request.resume_declared(
            roots::Root {
                directory: Path::new("/ws"),
                declaration: Some(&declaration),
            },
            &mut warnings,
            &mut session,
        )?;
        assert!(matches!(view.get(Field::Path), Some(Value::Text("../dep"))));
        assert_eq!(warnings.as_slice().len(), 1);
        Ok(())
    })();
    if result.is_err() {
        // A failed request cannot resume with a replacement account or allocate
        // via a later operation on the original session.
        assert!(session.check().is_err());
        assert!(session.items(1).is_err());
    }
    session.finish(result)
}

#[test]
fn every_admission_refusal_preserves_original_account_and_stops() {
    let mut full = Account::default();
    run(&mut full).unwrap();
    for refusal in 0..full.requests.len() {
        let mut account = Account {
            refusal: Some(refusal),
            ..Account::default()
        };
        assert!(
            matches!(
                run(&mut account),
                Err(Error::Admission("inheritance admission"))
            ),
            "request {refusal}"
        );
        assert_eq!(
            account.requests,
            full.requests[..=refusal],
            "request {refusal}"
        );
    }
}

#[test]
fn exact_and_one_short_limits_cover_rebase_and_warning_with_retained_prefix() {
    let mut full = Account::default();
    run(&mut full).unwrap();
    assert!(full.used.iter().all(|count| *count > 0));
    let mut exact = Account {
        caps: Some(full.used),
        ..Account::default()
    };
    run(&mut exact).unwrap();
    assert_eq!(exact.requests, full.requests);
    for dimension in 0..3 {
        let mut caps = full.used;
        caps[dimension] -= 1;
        let mut short = Account {
            caps: Some(caps),
            ..Account::default()
        };
        assert!(
            matches!(
                run(&mut short),
                Err(Error::Admission("inheritance admission"))
            ),
            "dimension {dimension}"
        );
    }
}
