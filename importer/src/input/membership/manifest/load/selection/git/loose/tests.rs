use rawr_toml::document;

use crate::input::membership::manifest::{reference::Account, session::Session};

use super::{decode, name, Candidate, Failure, Target};

#[path = "reference.rs"]
#[allow(dead_code)] // Keep the uncalled sanitize branch in the pinned reference.
mod pinned;

fn reference(input: &[u8]) -> Result<(), name::Failure> {
    pinned::name_inner(input, pinned::Mode::Validate)?;
    if !input.contains(&b'/') && !input.iter().all(|c| c.is_ascii_uppercase() || *c == b'_') {
        return Err(name::Failure::SomeLowercase);
    }
    Ok(())
}

#[test]
fn complete_names_match_pinned_validation_and_error_precedence() {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut cases: Vec<Vec<u8>> = [
        b"".as_slice(),
        b"/",
        b"//",
        b"a/",
        b"/a",
        b"a//b",
        b"a..b",
        b"a/.b",
        b".a",
        b"a.",
        b".lock",
        b"a.lock",
        b".lock/a",
        b"a.lock/b",
        b"a@{b",
        b"a*b",
        b"refs/heads/HEAD",
        b"HEAD",
        b"HEAD_",
        b"HEAD1",
        b"refs/heads/\xff",
        b"refs/heads/.a.lock",
        b"refs/heads/a.lock/",
        b"a..b?",
        b"a?b..",
    ]
    .iter()
    .map(|input| input.to_vec())
    .collect();
    for byte in 0..=255 {
        cases.push(vec![byte]);
        cases.push([b"refs/heads/".as_slice(), &[byte], b"valid"].concat());
    }
    for input in cases {
        assert_eq!(
            name::validate(&input, &mut session).unwrap(),
            reference(&input),
            "{input:?}"
        );
    }
    session.finish(Ok(())).unwrap();
}

#[test]
fn loose_decode_preserves_hash_width_delimiters_and_owned_failures() {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let hex = b"0123456789abcdefABCDEF0123456789abcdefAB";
    let expected = [
        0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67,
        0x89, 0xab, 0xcd, 0xef, 0xab,
    ];
    for candidate in Candidate::ALL {
        for suffix in [b"".as_slice(), b"\n", b"\r\n", b" xyz", b"g", b"\0"] {
            let contents = [hex.as_slice(), suffix].concat();
            let result = decode(candidate, &contents, &mut session).unwrap().unwrap();
            assert_eq!(result.name, candidate.bytes());
            assert_eq!(result.target, Target::Object(expected));
        }
    }
    for contents in [
        b"".to_vec(),
        b"not a reference".to_vec(),
        hex[..39].to_vec(),
        [hex.as_slice(), b"a"].concat(),
        vec![b'0'; 64],
    ] {
        assert_eq!(
            decode(Candidate::Head, &contents, &mut session).unwrap(),
            Err(Failure::Parse { content: contents })
        );
    }
    for contents in [
        b"ref: refs/heads/main".as_slice(),
        b"ref:   refs/heads/main\0ignored",
        b"ref: refs/heads/main\rignored",
        b"ref: refs/heads/main\nignored",
    ] {
        let result = decode(Candidate::Head, contents, &mut session)
            .unwrap()
            .unwrap();
        assert_eq!(result.target, Target::Symbolic(b"refs/heads/main".to_vec()));
    }
    let result = decode(Candidate::Head, b"ref: refs/heads/\xff", &mut session)
        .unwrap()
        .unwrap();
    assert_eq!(result.target, Target::Symbolic(b"refs/heads/\xff".to_vec()));
    assert_eq!(
        decode(Candidate::Head, b"ref:  \n", &mut session).unwrap(),
        Err(Failure::RefnameValidation {
            source: name::Failure::Empty,
            path: vec![]
        })
    );
    assert_eq!(
        decode(Candidate::Head, b"ref: refs/heads/bad?\n", &mut session).unwrap(),
        Err(Failure::RefnameValidation {
            source: name::Failure::InvalidByte { byte: vec![b'?'] },
            path: b"refs/heads/bad?".to_vec()
        })
    );
    session.finish(Ok(())).unwrap();
}

#[test]
fn invalid_byte_owns_its_payload_and_refusal_cannot_become_parse_failure() {
    struct Account {
        allocations: Vec<usize>,
        refuse: bool,
        calls: usize,
    }
    impl document::Admission for Account {
        type Error = ();
        fn admit(&mut self, request: document::Request) -> Result<(), ()> {
            self.calls += 1;
            if let document::Request::Allocation { bytes, .. } = request {
                self.allocations.push(bytes);
                if self.refuse && self.allocations.len() == 3 {
                    return Err(());
                }
            }
            Ok(())
        }
    }
    for refuse in [false, true] {
        let mut account = Account {
            allocations: vec![],
            refuse,
            calls: 0,
        };
        let mut session = Session::new(&mut account);
        let result = decode(Candidate::Head, b"ref: refs/heads/bad?", &mut session);
        if refuse {
            assert!(result.is_err());
            assert!(decode(Candidate::Head, b"", &mut session).is_err());
            assert!(session.finish(result).is_err());
        } else {
            assert!(matches!(
                session.finish(result).unwrap(),
                Err(Failure::RefnameValidation { .. })
            ));
        }
        assert_eq!(account.allocations, [4, 15, 1]);
    }
}

#[test]
fn git_read_live_codec_replay_reaches_loose_decoder() {
    use crate::input::membership::{host, live, stored, Budget};
    use crate::{cargo, input, node};

    for contents in [b"ref: refs/heads/main\n".as_slice(), b"broken"] {
        let record = live::git_read::tests::head(contents);
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::new(&limits);
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let image = stored::Stored::from_records(&[record], &mut budget).unwrap();
        let encoded = image.encode(&mut budget).unwrap();
        let image = stored::Stored::decode(&encoded, &mut budget).unwrap();
        let mut host = host::Host::from_image(&image, budget).unwrap();
        let mut session = Session::new(&mut host);
        let result = super::read::one(
            std::path::Path::new("/fixture"),
            Candidate::Head.name(),
            &mut session,
        );
        let result = session.finish(result).unwrap();
        if contents == b"broken" {
            assert_eq!(
                result,
                Err(super::read::Failure::ReferenceCreation {
                    source: Failure::Parse {
                        content: contents.to_vec()
                    },
                    relative_path: "HEAD".into(),
                })
            );
        } else {
            let reference = result.unwrap().unwrap();
            assert_eq!(reference.name, b"HEAD");
            assert_eq!(
                reference.target,
                Target::Symbolic(b"refs/heads/main".to_vec())
            );
        }
        host.finish().unwrap();
    }
}
