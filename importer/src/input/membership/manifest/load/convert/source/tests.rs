use super::*;
use rawr_toml::document;
#[derive(Default)]
struct Account {
    requests: Vec<document::Request>,
    refusal: Option<usize>,
}
impl document::Admission for Account {
    type Error = usize;
    fn admit(&mut self, request: document::Request) -> Result<(), usize> {
        let n = self.requests.len();
        self.requests.push(request);
        if self.refusal == Some(n) {
            Err(n)
        } else {
            Ok(())
        }
    }
}
fn add<'a>(
    owner: &mut Owner<'a>,
    text: &str,
    precise: Option<Precise<'_>>,
    label: Option<Label<'_>>,
    session: &mut Session<'_, Account>,
) -> Step<Id<'a>> {
    let url = Url::parse(text, session)?.unwrap();
    owner
        .intern(url, Origin::Registry, precise, label, session)
        .map(|value| value.ok().unwrap())
}
#[test]
fn incumbent_whole_representation_and_precise_identity_are_distinct() {
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let mut history = History::new();
    let mut owner = history.open(&mut session).unwrap();
    let first = add(
        &mut owner,
        "https://github.com/Team/Repo.git",
        None,
        None,
        &mut session,
    )
    .unwrap();
    let again = add(
        &mut owner,
        "https://github.com/team/repo",
        None,
        Some(Label::Registry("later")),
        &mut session,
    )
    .unwrap();
    assert!(first == again);
    let record = owner.get(again, &mut session).unwrap();
    assert_eq!(
        record
            .url(&mut session)
            .unwrap()
            .as_str(&mut session)
            .unwrap(),
        "https://github.com/Team/Repo.git"
    );
    assert_eq!(record.label(), None);
    let locked = add(
        &mut owner,
        "https://github.com/team/repo",
        Some(Precise::Locked),
        Some(Label::Registry("locked")),
        &mut session,
    )
    .unwrap();
    assert!(first != locked);
    assert_eq!(
        owner.public_cmp(first, locked, &mut session).unwrap(),
        Ordering::Equal
    );
    let v = crate::input::membership::manifest::scalar::Version::parse("1.2.3+one").unwrap();
    let w = crate::input::membership::manifest::scalar::Version::parse("1.2.3+two").unwrap();
    let a = add(
        &mut owner,
        "https://github.com/team/repo",
        Some(Precise::Updated {
            name: "pkg",
            from: v,
            to: v,
        }),
        None,
        &mut session,
    )
    .unwrap();
    let b = add(
        &mut owner,
        "https://github.com/team/repo",
        Some(Precise::Updated {
            name: "pkg",
            from: v,
            to: w,
        }),
        None,
        &mut session,
    )
    .unwrap();
    assert!(a != b);
    assert_eq!(
        owner.public_cmp(a, b, &mut session).unwrap(),
        Ordering::Equal
    );
    session.finish(Ok(())).unwrap();
}
#[test]
fn sorted_index_preserves_insertion_identity_and_sparse_kind_is_derived() {
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let mut history = History::new();
    let mut owner = history.open(&mut session).unwrap();
    for i in (0..40).rev() {
        add(
            &mut owner,
            &format!("https://example.com/{i:02}"),
            None,
            None,
            &mut session,
        )
        .unwrap();
    }
    for pair in owner.index.as_slice().windows(2) {
        assert_eq!(
            compare(pair[0], pair[1], true, &mut session).unwrap(),
            Ordering::Less
        );
    }
    let id = add(
        &mut owner,
        "sparse+https://example.com/index/",
        None,
        None,
        &mut session,
    )
    .unwrap();
    assert_eq!(
        owner.get(id, &mut session).unwrap().kind(),
        Kind::SparseRegistry
    );
    let url = Url::parse("https://example.com/repo", &mut session)
        .unwrap()
        .unwrap();
    let id = owner
        .intern(
            url,
            Origin::Git(Git::Branch("a%2Fb+name")),
            None,
            None,
            &mut session,
        )
        .unwrap()
        .ok()
        .unwrap();
    assert_eq!(
        owner.get(id, &mut session).unwrap().kind(),
        Kind::Git(Git::Branch("a%2Fb+name"))
    );
    session.finish(Ok(())).unwrap();
}
#[test]
fn foreign_owner_handles_never_alias_or_pass_access() {
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let mut left = History::new();
    let mut right = History::new();
    let mut a = left.open(&mut session).unwrap();
    let mut b = right.open(&mut session).unwrap();
    let x = add(&mut a, "https://example.com/", None, None, &mut session).unwrap();
    let y = add(&mut b, "https://example.com/", None, None, &mut session).unwrap();
    assert!(x != y);
    assert!(b.get(x, &mut session).is_err());
    assert!(session.finish(Ok(())).is_err());
}
fn exercise(owner: &mut Owner<'_>, session: &mut Session<'_, Account>) -> Step<()> {
    for text in [
        "https://github.com/Z/repo.git",
        "https://github.com/A/repo",
        "https://github.com/z/repo",
        "https://github.com/M/repo",
    ] {
        add(
            owner,
            text,
            Some(Precise::GitUrlFragment("abc")),
            Some(Label::Source("first")),
            session,
        )?;
    }
    Ok(())
}
#[test]
fn refusal_during_candidate_lookup_or_insertion_latches_owner_across_sessions() {
    let mut full = Account::default();
    let mut session = Session::new(&mut full);
    let mut history = History::new();
    let mut owner = history.open(&mut session).unwrap();
    let result = exercise(&mut owner, &mut session);
    session.finish(result).unwrap();
    for refusal in 1..full.requests.len() {
        let mut account = Account {
            refusal: Some(refusal),
            ..Account::default()
        };
        let mut session = Session::new(&mut account);
        let mut history = History::new();
        let mut owner = history.open(&mut session).unwrap();
        assert!(exercise(&mut owner, &mut session).is_err(), "{refusal}");
        assert!(session.finish(Ok(())).is_err());
        assert_eq!(account.requests, full.requests[..=refusal]);
        // Parse refusal precedes intern entry and cannot taint an unused owner.
        // Once intern starts, its latch must survive replacing the Session.
        if owner.refused.get() {
            let mut fresh = Account::default();
            let mut session = Session::new(&mut fresh);
            let url = Url::parse("https://next/", &mut session).unwrap().unwrap();
            assert!(owner
                .intern(url, Origin::Registry, None, None, &mut session)
                .is_err());
            assert!(session.finish(Ok(())).is_err());
        }
    }
}

#[test]
fn identical_public_handle_comparison_has_no_key_admissions() {
    let mut account = Account::default();
    let mut history = History::new();
    let mut session = Session::new(&mut account);
    let mut owner = history.open(&mut session).unwrap();
    let id = add(
        &mut owner,
        "https://example.com/long",
        None,
        None,
        &mut session,
    )
    .unwrap();
    session.finish(Ok(())).unwrap();
    let mut fresh = Account {
        refusal: Some(0),
        ..Account::default()
    };
    let mut session = Session::new(&mut fresh);
    assert_eq!(
        owner.public_cmp(id, id, &mut session).unwrap(),
        Ordering::Equal
    );
    session.finish(Ok(())).unwrap();
    assert!(fresh.requests.is_empty());
}

#[test]
fn public_comparison_refusal_prevents_reuse_with_a_fresh_session() {
    fn run(refusal: Option<usize>) -> Vec<document::Request> {
        let mut setup = Account::default();
        let mut history = History::new();
        let mut session = Session::new(&mut setup);
        let mut owner = history.open(&mut session).unwrap();
        let left = add(
            &mut owner,
            "https://example.com/a",
            None,
            None,
            &mut session,
        )
        .unwrap();
        let right = add(
            &mut owner,
            "https://example.com/b",
            None,
            None,
            &mut session,
        )
        .unwrap();
        session.finish(Ok(())).unwrap();

        let mut account = Account {
            refusal,
            ..Account::default()
        };
        let mut session = Session::new(&mut account);
        let compared = owner.public_cmp(left, right, &mut session);
        if let Some(at) = refusal {
            assert!(compared.is_err());
            assert!(
                matches!(session.finish(compared), Err(Error::Admission(index)) if index == at)
            );
            let mut fresh = Account::default();
            let mut session = Session::new(&mut fresh);
            assert!(owner.get(left, &mut session).is_err());
            assert!(session.finish(Ok(())).is_err());
            assert!(fresh.requests.is_empty());
        } else {
            assert_eq!(session.finish(compared).unwrap(), Ordering::Less);
        }
        account.requests
    }
    let baseline = run(None);
    assert!(!baseline.is_empty());
    for refusal in 0..baseline.len() {
        assert_eq!(run(Some(refusal)), baseline[..=refusal]);
    }
}
