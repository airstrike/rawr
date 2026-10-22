use super::*;

const FIRST: Identity = Identity {
    device: 7,
    inode: 11,
};

#[test]
fn source_branch_matrix_selects_spelling_and_diagnostic_intent() {
    let distinct_device = Identity { device: 8, ..FIRST };
    let distinct_inode = Identity { inode: 12, ..FIRST };
    for filename in [Filename::Config, Filename::Credentials] {
        for warn in [false, true] {
            let acquired = Attempt::Acquired(FIRST);
            if warn {
                for (second, expected) in [
                    (Attempt::Acquired(FIRST), Diagnostic::None),
                    (Attempt::Acquired(distinct_device), Diagnostic::Both),
                    (Attempt::Acquired(distinct_inode), Diagnostic::Both),
                    (
                        Attempt::OpenFailed(NativeError::Os(13)),
                        Diagnostic::Deprecated,
                    ),
                    (Attempt::StatFailed(5), Diagnostic::Deprecated),
                ] {
                    let after = After::Second(second);
                    let transcript = Transcript::checked(filename, warn, acquired, after).unwrap();
                    assert_eq!(transcript.selection(), Selection::Legacy);
                    assert_eq!(transcript.diagnostic(), expected);
                    assert_eq!(transcript.filename(), filename);
                    assert_eq!(transcript.warn(), warn);
                    assert_eq!(transcript.legacy(), acquired);
                    assert_eq!(transcript.after(), after);
                }
            } else {
                let transcript =
                    Transcript::checked(filename, warn, acquired, After::None).unwrap();
                assert_eq!(transcript.selection(), Selection::Legacy);
                assert_eq!(transcript.diagnostic(), Diagnostic::None);
            }
            for legacy in [
                Attempt::OpenFailed(NativeError::Os(13)),
                Attempt::StatFailed(5),
            ] {
                for (exists, selection) in [
                    (Exists::Present, Selection::Toml),
                    (Exists::Failed(NativeError::Os(2)), Selection::None),
                    (Exists::Failed(NativeError::Os(13)), Selection::None),
                ] {
                    let transcript =
                        Transcript::checked(filename, warn, legacy, After::Fallback(exists))
                            .unwrap();
                    assert_eq!(transcript.selection(), selection);
                    assert_eq!(transcript.diagnostic(), Diagnostic::None);
                }
            }
            let transcript = Transcript::checked(
                filename,
                warn,
                Attempt::OpenFailed(NativeError::InvalidInput),
                After::Fallback(Exists::Failed(NativeError::InvalidInput)),
            )
            .unwrap();
            assert_eq!(transcript.selection(), Selection::None);
            assert_eq!(transcript.diagnostic(), Diagnostic::None);
        }
    }
    assert_eq!(Filename::Config.legacy(), "config");
    assert_eq!(Filename::Config.toml(), "config.toml");
    assert_eq!(Filename::Credentials.legacy(), "credentials");
    assert_eq!(Filename::Credentials.toml(), "credentials.toml");
}

#[test]
fn checked_transcripts_reject_unreached_branches_and_invalid_native_failures() {
    let attempts = [
        Attempt::Acquired(FIRST),
        Attempt::OpenFailed(NativeError::Os(13)),
        Attempt::OpenFailed(NativeError::InvalidInput),
        Attempt::StatFailed(5),
        Attempt::OpenFailed(NativeError::Os(0)),
        Attempt::OpenFailed(NativeError::Os(-1)),
        Attempt::StatFailed(0),
        Attempt::StatFailed(-1),
    ];
    let fallbacks = [
        Exists::Present,
        Exists::Failed(NativeError::Os(2)),
        Exists::Failed(NativeError::InvalidInput),
        Exists::Failed(NativeError::Os(0)),
        Exists::Failed(NativeError::Os(-1)),
    ];
    // This expected matrix is expressed as permitted category indices, rather
    // than calling the implementation's validation predicates.
    for filename in [Filename::Config, Filename::Credentials] {
        for warn in [false, true] {
            for (index, legacy) in attempts.into_iter().enumerate() {
                assert_eq!(
                    Transcript::checked(filename, warn, legacy, After::None).is_some(),
                    index == 0 && !warn,
                );
                for (second_index, second) in attempts.into_iter().enumerate() {
                    assert_eq!(
                        Transcript::checked(filename, warn, legacy, After::Second(second))
                            .is_some(),
                        index == 0 && warn && [0, 1, 3].contains(&second_index),
                        "{filename:?} {warn} {legacy:?} {second:?}",
                    );
                }
                for (fallback_index, fallback) in fallbacks.into_iter().enumerate() {
                    let expected = match index {
                        1 | 3 => [0, 1].contains(&fallback_index),
                        2 => fallback_index == 2,
                        _ => false,
                    };
                    assert_eq!(
                        Transcript::checked(filename, warn, legacy, After::Fallback(fallback))
                            .is_some(),
                        expected,
                        "{filename:?} {warn} {legacy:?} {fallback:?}",
                    );
                }
            }
        }
    }
}
