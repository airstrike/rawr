use std::{alloc::Layout, path::Path};

use rawr_toml::document::{Admission, Request};

use super::super::Builder;
use super::{paths, rule};
use crate::input::membership::manifest::{
    reference::Account, session::Session, storage::Buffer, Error, Stop,
};

#[derive(Default)]
struct Count {
    requests: Vec<Request>,
    refuse: Option<usize>,
}
impl Admission for Count {
    type Error = usize;
    fn admit(&mut self, request: Request) -> Result<(), usize> {
        let index = self.requests.len();
        self.requests.push(request);
        if self.refuse == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

fn builder() -> Builder {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut builder = Builder::prepare(Path::new("./root"), &mut session).unwrap();
    for text in ["name", "!cache/"] {
        assert!(builder.add(text, &mut session).unwrap().is_ok());
    }
    builder
}

#[test]
fn borrowed_build_retains_independent_owned_metadata_and_matcher_after_builder_drop() {
    let mut builder = builder();
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut first = builder
        .build(&mut session)
        .unwrap()
        .ok()
        .expect("first matcher");
    assert_eq!((first.num_ignores, first.num_whitelists), (1, 1));
    assert_eq!(first.set.len(), 2);
    assert!(first.matches.is_none());
    for (owned, original) in first.globs.iter().zip(builder.rules.originals.as_slice()) {
        assert_eq!(
            (
                &owned.original,
                &owned.actual,
                owned.whitelist,
                owned.directory
            ),
            (
                &original.original,
                &original.actual,
                original.whitelist,
                original.directory
            )
        );
        assert_ne!(owned.original.as_ptr(), original.original.as_ptr());
        assert_ne!(owned.actual.as_ptr(), original.actual.as_ptr());
    }
    // build borrows the real builder; later source additions cannot mutate the
    // already-owned engine or its cloned reporting metadata.
    assert!(builder.add("later", &mut session).unwrap().is_ok());
    let mut second = builder
        .build(&mut session)
        .unwrap()
        .ok()
        .expect("second matcher");
    assert_eq!((second.num_ignores, second.num_whitelists), (2, 1));
    assert_eq!(second.set.len(), 3);
    assert!(second.matches.is_none());
    drop(builder);
    assert_eq!(first.globs.len(), 2);
    assert_eq!(first.globs[1].original, "!cache/");
    assert!(first.globs[1].whitelist && first.globs[1].directory);
    let path = first
        .root
        .strip(Path::new("./root/later"), &mut session)
        .unwrap();
    assert_eq!(path, Path::new("later"));
    let candidate = paths::Candidate::prepare(path, &mut session).unwrap();
    let mut found = Buffer::new();
    first
        .set
        .matches_into(&candidate, &mut found, &mut session)
        .unwrap();
    assert!(found.as_slice().is_empty());
    second
        .set
        .matches_into(&candidate, &mut found, &mut session)
        .unwrap();
    assert_eq!(found.as_slice(), [2]);
    // Inner matching cannot eagerly acquire the distinct ignore scratch.
    assert!(first.matches.is_none() && second.matches.is_none());
}

#[test]
fn exact_metadata_clone_is_admitted_before_elements_and_refusal_keeps_builder() {
    let builder = builder();
    let mut complete = Count::default();
    drop(
        builder
            .build(&mut Session::new(&mut complete))
            .unwrap()
            .ok()
            .expect("matcher"),
    );
    // The two source filters visit every rule before any GlobSet work.
    assert_eq!(&complete.requests[..4], [Request::Items(1); 4]);
    let layout = Layout::array::<rule::Prepared>(2).unwrap();
    let allocation = Request::Allocation {
        bytes: layout.size(),
        alignment: layout.align(),
    };
    let gate = complete
        .requests
        .iter()
        .rposition(|request| *request == allocation)
        .expect("metadata Vec");
    assert_eq!(
        &complete.requests[gate + 1..gate + 9],
        &[
            Request::Items(1),
            Request::String(4),
            Request::Items(1),
            Request::Allocation {
                bytes: 4,
                alignment: 1
            },
            Request::String(7),
            Request::Items(1),
            Request::Allocation {
                bytes: 7,
                alignment: 1
            },
            Request::Items(1),
        ]
    );
    for refuse in [gate, gate + 5] {
        let mut count = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        let mut session = Session::new(&mut count);
        assert!(matches!(builder.build(&mut session), Err(Stop::Refused)));
        assert!(matches!(builder.build(&mut session), Err(Stop::Refused)));
        assert!(
            matches!(session.finish::<()>(Err(Stop::Refused)), Err(Error::Admission(index)) if index == refuse)
        );
        assert_eq!(count.requests, complete.requests[..=refuse]);
        assert_eq!(builder.rules.originals.as_slice()[0].original, "name");
        assert_eq!(builder.rules.originals.as_slice()[0].actual, "**/name");
        assert_eq!(builder.rules.patterns.len(), 2);
    }
}
