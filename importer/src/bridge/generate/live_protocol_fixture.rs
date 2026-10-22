// Appended to the actual emitted framing and live_protocol source by its test.
// Compilation, UI, and wire digests are modeled; the emitted transition
// ownership and socket handling execute verbatim. The real codec has separate
// identity/digest tests in protocol::resident.
use std::os::unix::net::UnixStream;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
const COMPILED_SURFACE: [u8; 32] = [42; 32];
static ACCOUNT_REPORTS: AtomicUsize = AtomicUsize::new(0);
mod __rawr_internal {
    pub type Shared<T> = std::sync::Arc<T>;
    pub struct Engine {
        pub revision: super::Arc<super::AtomicUsize>,
        pub compiled: super::Arc<super::AtomicUsize>,
        pub eligible: super::Arc<super::AtomicBool>,
    }
    impl Engine {
        pub fn imported_native_snapshot(&self) -> Option<package::import::runner::account::Snapshot> {
            Some(package::import::runner::account::Snapshot(
                self.compiled.load(super::Ordering::SeqCst),
            ))
        }
    }
    pub mod reload {
        pub struct Candidate(pub usize);
    }
    pub mod package {
        pub mod snapshot {
            pub struct Snapshot(pub u8);
        }
        pub mod import {
            pub mod runner {
                pub mod account {
                    #[derive(Clone, Copy)]
                    pub struct Snapshot(pub usize);
                }
            }
            pub mod resident {
                pub struct Semantic([u8; 32]);
                impl Semantic {
                    pub fn as_bytes(&self) -> &[u8; 32] {
                        &self.0
                    }
                }
                pub struct Compiled(pub crate::__rawr_internal::reload::Candidate);
                impl Compiled {
                    pub fn into_parts(
                        self,
                    ) -> (
                        crate::__rawr_internal::reload::Candidate,
                        Option<Semantic>,
                        [u8; 32],
                        [u8; 32],
                    ) {
                        (self.0, Some(Semantic([21; 32])), [22; 32], [23; 32])
                    }
                }
                pub enum Outcome {
                    Compiled(Compiled),
                    NeedsPreparation,
                }
                pub enum Error {
                    Source(&'static str),
                    Resource(&'static str),
                    Unowned,
                }
                impl std::fmt::Display for Error {
                    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        f.write_str("fixture compiler error")
                    }
                }
                pub mod wire {
                    pub struct Attempt([u8; 100]);
                    pub struct Compiled([u8; 197]);
                    impl Attempt {
                        pub fn decode<'a>(
                            payload: &'a [u8],
                            original: &[u8; 56],
                        ) -> std::io::Result<(Self, &'a [u8])> {
                            let (header, snapshot) = payload
                                .split_first_chunk::<100>()
                                .ok_or_else(|| std::io::Error::other("fixture framing"))?;
                            if header[..4] != 1_u32.to_le_bytes()
                                || header[4..60] != *original
                                || header[60..68] == [0; 8]
                                || snapshot.len() != 1
                                || header[68..] != [snapshot[0]; 32]
                            {
                                return Err(std::io::Error::other("fixture correlation"));
                            }
                            Ok((Self(*header), snapshot))
                        }
                        pub fn response_header(&self) -> &[u8] {
                            &self.0
                        }
                        pub fn compiled(
                            self,
                            carrier: [u8; 32],
                            coverage: [u8; 32],
                            semantic: Option<[u8; 32]>,
                        ) -> Compiled {
                            let mut bytes = [0; 197];
                            bytes[..100].copy_from_slice(&self.0);
                            bytes[100..132].copy_from_slice(&carrier);
                            bytes[132..164].copy_from_slice(&coverage);
                            if let Some(semantic) = semantic {
                                bytes[164] = 1;
                                bytes[165..].copy_from_slice(&semantic);
                            }
                            Compiled(bytes)
                        }
                    }
                    impl Compiled {
                        pub fn key(&self) -> [u8; 32] {
                            [self.0[36] + 10; 32]
                        }
                        pub fn encode(&self) -> &[u8] {
                            &self.0
                        }
                        pub fn advance_receipt(&self, previous: [u8; 32]) -> [u8; 32] {
                            previous.map(|byte| byte ^ self.key()[0])
                        }
                    }
                }
            }
        }
    }
}

// The exact codec is tested in protocol::account. This fixture models its fixed
// payload while executing the emitted reporting and original-error helper.
fn account_payload(
    tag: u8,
    request: &[u8],
    accounts: [Option<__rawr_internal::package::import::runner::account::Snapshot>; 2],
) -> Option<[u8; 252]> {
    if !matches!(tag, 3 | 7 | 9 | 12 | 20 | 22 | 25 | 26 | 31 | 37) {
        return None;
    }
    let mut bytes = [0; 252];
    bytes[0] = 1;
    bytes[1] = tag;
    bytes[2..10].copy_from_slice(&(request.len() as u64).to_le_bytes());
    let prefix = request.len().min(56);
    bytes[10..10 + prefix].copy_from_slice(&request[..prefix]);
    for (index, account) in accounts.into_iter().enumerate() {
        if let Some(account) = account {
            bytes[66 + index * 93] = 1;
            bytes[67 + index * 93..75 + index * 93]
                .copy_from_slice(&(account.0 as u64).to_le_bytes());
        }
    }
    Some(bytes)
}

fn receive_response(stream: &mut UnixStream) -> std::io::Result<(u8, Vec<u8>)> {
    let mut reports = 0;
    loop {
        let frame = receive_frame(stream)?;
        if frame.0 != 45 {
            return Ok(frame);
        }
        reports += 1;
        assert_eq!(reports, 1, "one account frame per existing completion");
        assert_eq!(frame.1.len(), 252);
        ACCOUNT_REPORTS.fetch_add(1, Ordering::SeqCst);
    }
}

mod __rawr_profile {
    use super::*;
    pub struct ApplicationContract;
    pub struct PendingInstall(usize);
    pub struct Transport {
        pub revision: Arc<AtomicUsize>,
        pub installed: Arc<AtomicUsize>,
        pub cancelled: Arc<AtomicUsize>,
        pub candidate: Arc<AtomicUsize>,
        pub eligible: Arc<AtomicBool>,
        pub revoke_on_eligibility: Arc<AtomicBool>,
        pub refresh: Arc<AtomicBool>,
    }
    #[derive(Debug)]
    pub struct InstallError;
    impl std::fmt::Display for InstallError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("obsolete")
        }
    }
    impl InstallError {
        pub fn not_installed_diagnostic(&self) -> Option<&str> {
            Some("obsolete")
        }
        pub fn is_superseded(&self) -> bool {
            false
        }
    }
    impl Transport {
        pub fn take_refresh(&self) -> bool {
            self.refresh.swap(false, Ordering::SeqCst)
        }
        pub fn export_state(&self) -> Option<Vec<u8>> {
            None
        }
        pub fn begin_install(&self) -> Result<PendingInstall, InstallError> {
            Ok(PendingInstall(
                self.revision.fetch_add(1, Ordering::SeqCst) + 1,
            ))
        }
        pub fn observe_install(
            &self,
            _: &mut PendingInstall,
            bytes: &[u8],
        ) -> Result<(), InstallError> {
            if bytes != [1] {
                return Err(InstallError);
            }
            Ok(())
        }
        pub fn cancel_install(&self, _: PendingInstall) -> Result<(), InstallError> {
            self.cancelled.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        pub fn request_is_current(&self, pending: &PendingInstall) -> Result<bool, InstallError> {
            Ok(pending.0 == self.revision.load(Ordering::SeqCst))
        }
        pub fn resident_eligible(&self, _: &PendingInstall) -> bool {
            if self.revoke_on_eligibility.swap(false, Ordering::SeqCst) {
                self.eligible.store(false, Ordering::SeqCst);
                self.revision.fetch_add(1, Ordering::SeqCst);
            }
            self.eligible.load(Ordering::SeqCst)
        }
        pub fn install(
            &self,
            pending: PendingInstall,
            candidate: __rawr_internal::reload::Candidate,
        ) -> Result<(), InstallError> {
            if pending.0 != self.revision.load(Ordering::SeqCst) {
                return Err(InstallError);
            }
            self.candidate.store(candidate.0, Ordering::SeqCst);
            self.installed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        pub fn report(&self, _: &str) -> std::io::Result<()> {
            Ok(())
        }
        pub fn exit(&self) -> std::io::Result<()> {
            Ok(())
        }
        pub fn retire(&self) -> std::io::Result<bool> {
            self.exit()?;
            Ok(self.take_refresh())
        }
    }
    pub fn compile_resident(
        engine: &__rawr_internal::Engine,
        snapshot: &__rawr_internal::package::snapshot::Snapshot,
        _: &ApplicationContract,
    ) -> Result<
        __rawr_internal::package::import::resident::Outcome,
        __rawr_internal::package::import::resident::Error,
    > {
        use __rawr_internal::package::import::resident::{Compiled, Error, Outcome};
        let serial = engine.compiled.fetch_add(1, Ordering::SeqCst) + 1;
        match snapshot.0 {
            1 => return Ok(Outcome::NeedsPreparation),
            2 => return Err(Error::Source("source diagnostic")),
            3 => return Err(Error::Resource("native resource limit")),
            4 => {
                engine.revision.fetch_add(1, Ordering::SeqCst);
            }
            5 => {
                engine.eligible.store(false, Ordering::SeqCst);
                engine.revision.fetch_add(1, Ordering::SeqCst);
            }
            6 => return Err(Error::Unowned),
            7 | 8 => {
                // Even without a gate revision, a typed compiler failure cannot
                // be rewritten to a native miss by a late eligibility change.
                engine.eligible.store(false, Ordering::SeqCst);
                return Err(if snapshot.0 == 7 {
                    Error::Source("source diagnostic")
                } else {
                    Error::Resource("native resource limit")
                });
            }
            _ => {}
        }
        Ok(Outcome::Compiled(Compiled(
            __rawr_internal::reload::Candidate(serial),
        )))
    }
}
enum Error {
    Stale,
}
fn invoke_rejection(_: &[u8; 32], _: &Error) -> Vec<u8> {
    vec![]
}
fn compile_live(
    _: &__rawr_internal::Shared<__rawr_internal::Engine>,
    _: &__rawr_profile::ApplicationContract,
    bytes: &[u8],
    _: &mut impl FnMut(u8) -> std::io::Result<()>,
) -> std::io::Result<Option<__rawr_internal::reload::Candidate>> {
    if bytes == [1] {
        return Err(std::io::Error::other("compile failed"));
    }
    Ok(Some(__rawr_internal::reload::Candidate(0)))
}
fn decode_snapshot(bytes: &[u8]) -> std::io::Result<__rawr_internal::package::snapshot::Snapshot> {
    match bytes {
        [scenario] => Ok(__rawr_internal::package::snapshot::Snapshot(*scenario)),
        _ => Err(std::io::Error::other("fixture snapshot")),
    }
}
struct Session {
    client: UnixStream,
    server: std::thread::JoinHandle<std::io::Result<()>>,
    revision: Arc<AtomicUsize>,
    installed: Arc<AtomicUsize>,
    cancelled: Arc<AtomicUsize>,
    candidate: Arc<AtomicUsize>,
    compiled: Arc<AtomicUsize>,
    eligible: Arc<AtomicBool>,
    revoke_on_eligibility: Arc<AtomicBool>,
    refresh: Arc<AtomicBool>,
}
fn session() -> Session {
    let (client, server) = UnixStream::pair().unwrap();
    for stream in [&client, &server] {
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        stream
            .set_write_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
    }
    let revision = Arc::new(AtomicUsize::new(0));
    let installed = Arc::new(AtomicUsize::new(0));
    let cancelled = Arc::new(AtomicUsize::new(0));
    let candidate = Arc::new(AtomicUsize::new(0));
    let compiled = Arc::new(AtomicUsize::new(0));
    let eligible = Arc::new(AtomicBool::new(true));
    let revoke_on_eligibility = Arc::new(AtomicBool::new(false));
    let refresh = Arc::new(AtomicBool::new(false));
    let engine = __rawr_internal::Engine {
        revision: revision.clone(),
        compiled: compiled.clone(),
        eligible: eligible.clone(),
    };
    let transport = __rawr_profile::Transport {
        revision: revision.clone(),
        installed: installed.clone(),
        cancelled: cancelled.clone(),
        candidate: candidate.clone(),
        eligible: eligible.clone(),
        revoke_on_eligibility: revoke_on_eligibility.clone(),
        refresh: refresh.clone(),
    };
    let server = std::thread::spawn(move || {
        live_protocol(
            server,
            [7; 32],
            [8; 32],
            Arc::new(engine),
            __rawr_profile::ApplicationContract,
            transport,
        )
    });
    Session {
        client,
        server,
        revision,
        installed,
        cancelled,
        candidate,
        compiled,
        eligible,
        revoke_on_eligibility,
        refresh,
    }
}
fn identity(sequence: u64) -> Vec<u8> {
    let mut bytes = vec![7; 32];
    bytes.extend_from_slice(&[sequence as u8; 16]);
    bytes.extend_from_slice(&sequence.to_le_bytes());
    bytes
}
fn exchange_test(s: &mut Session, tag: u8, payload: &[u8], response: u8) -> Vec<u8> {
    let before = ACCOUNT_REPORTS.load(Ordering::SeqCst);
    send_frame(&mut s.client, tag, payload).unwrap();
    let (actual, bytes) = receive_response(&mut s.client).unwrap();
    let expected = usize::from(matches!(tag, 3 | 7 | 9 | 12 | 20 | 22 | 25 | 26 | 31 | 37));
    assert_eq!(ACCOUNT_REPORTS.load(Ordering::SeqCst) - before, expected);
    assert_eq!(actual, response);
    bytes
}
fn begin(s: &mut Session, sequence: u64) {
    let id = identity(sequence);
    assert_eq!(exchange_test(s, 29, &id, 30), id);
}
fn observe(s: &mut Session, sequence: u64) {
    let id = identity(sequence);
    let mut payload = id.clone();
    payload.push(1);
    assert_eq!(exchange_test(s, 33, &payload, 34), id);
}
fn prepare(s: &mut Session, sequence: u64, fail: bool) -> Vec<u8> {
    let mut bytes = vec![9; 32];
    bytes.extend_from_slice(&COMPILED_SURFACE);
    bytes.extend_from_slice(&[10; 32]);
    bytes.extend_from_slice(&[sequence as u8; 16]);
    bytes.push(u8::from(fail));
    exchange_test(s, 9, &bytes, if fail { 16 } else { 4 })
}
fn resident_request(sequence: u64, scenario: u8) -> Vec<u8> {
    let mut bytes = 1_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&identity(sequence));
    bytes.extend_from_slice(&1_u64.to_le_bytes());
    bytes.extend_from_slice(&[scenario; 32]);
    bytes.push(scenario);
    bytes
}
fn resident(s: &mut Session, scenario: u8, expected: u8, compiled: bool) -> Vec<u8> {
    let request = resident_request(1, scenario);
    let before = ACCOUNT_REPORTS.load(Ordering::SeqCst);
    send_frame(&mut s.client, 37, &request).unwrap();
    let mut stages = Vec::new();
    loop {
        let (tag, bytes) = receive_response(&mut s.client).unwrap();
        if tag == 24 {
            assert_eq!(bytes.len(), 51);
            assert_eq!(&bytes[..32], &[7; 32]);
            assert_eq!(&bytes[32..48], &[1; 16]);
            assert_eq!(bytes[48], 3);
            assert_eq!(bytes[49] as usize, stages.len() + 1);
            stages.push(bytes[50]);
            continue;
        }
        assert_eq!(tag, expected);
        assert_eq!(ACCOUNT_REPORTS.load(Ordering::SeqCst) - before, 1);
        assert_eq!(&bytes[..100], &request[..100]);
        assert_eq!(stages, if compiled { vec![1, 4] } else { vec![1] });
        return bytes;
    }
}
fn resident_commit() -> Vec<u8> {
    let mut commit = vec![11; 32];
    commit.extend_from_slice(&[1; 16]);
    commit
}
fn stop(mut s: Session, key: u8) {
    assert_eq!(exchange_test(&mut s, 12, &[key; 32], 13), vec![key; 32]);
    s.server.join().unwrap().unwrap();
}
fn main() {
    let (mut sender, receiver) = UnixStream::pair().unwrap();
    drop(receiver);
    let error = account_result::<()>(
        Err(std::io::Error::other("original compiler refusal")),
        &mut sender,
        37,
        &[1; 100],
        [Some(__rawr_internal::package::import::runner::account::Snapshot(1)), None],
    ).unwrap_err();
    assert_eq!(error.to_string(), "original compiler refusal");
    // Each edit interval must consume the pending candidate without advancing
    // the old owner, and a subsequent fresh request must still succeed.
    for before_compile in [true, false] {
        let mut s = session();
        begin(&mut s, 1);
        if before_compile {
            s.revision.fetch_add(1, Ordering::SeqCst);
        }
        observe(&mut s, 1);
        let commit = prepare(&mut s, 1, false);
        if !before_compile {
            s.revision.fetch_add(1, Ordering::SeqCst);
        }
        let rejected = exchange_test(&mut s, 25, &commit, 28);
        assert_eq!(&rejected[..48], &commit);
        assert_eq!(s.installed.load(Ordering::SeqCst), 0);
        begin(&mut s, 2);
        observe(&mut s, 2);
        let commit = prepare(&mut s, 2, false);
        assert_eq!(exchange_test(&mut s, 25, &commit, 8), commit);
        assert_eq!(s.installed.load(Ordering::SeqCst), 1);
        stop(s, 9);
    }
    // Local parent failure and child compile failure both have one exact cancel.
    for compile_failure in [false, true] {
        let mut s = session();
        begin(&mut s, 1);
        if compile_failure {
            observe(&mut s, 1);
            prepare(&mut s, 1, true);
        }
        let id = identity(1);
        assert_eq!(exchange_test(&mut s, 31, &id, 32), id);
        begin(&mut s, 2);
        observe(&mut s, 2);
        let commit = prepare(&mut s, 2, false);
        assert_eq!(exchange_test(&mut s, 26, &commit, 27), commit);
        assert_eq!(s.installed.load(Ordering::SeqCst), 0);
        stop(s, 7);
    }
    // Replaying a cancelled begin, foreign cancellation, and malformed begin
    // are terminal; none can mint a replacement request or install anything.
    for scenario in 0..3 {
        let mut s = session();
        begin(&mut s, 1);
        let mut id = identity(1);
        let tag = match scenario {
            0 => {
                exchange_test(&mut s, 31, &id, 32);
                29
            }
            1 => {
                id[32] ^= 1;
                31
            }
            _ => {
                exchange_test(&mut s, 31, &id, 32);
                id.pop();
                29
            }
        };
        send_frame(&mut s.client, tag, &id).unwrap();
        assert!(receive_response(&mut s.client).is_err());
        assert!(s.server.join().unwrap().is_err());
        assert_eq!(s.installed.load(Ordering::SeqCst), 0);
    }
    // A rejected observation leaves the early identity cancellable once.
    {
        let mut s = session();
        begin(&mut s, 1);
        let id = identity(1);
        let mut malformed = id.clone();
        malformed.push(0);
        exchange_test(&mut s, 33, &malformed, 16);
        assert_eq!(exchange_test(&mut s, 31, &id, 32), id);
        begin(&mut s, 2);
        observe(&mut s, 2);
        let commit = prepare(&mut s, 2, false);
        assert_eq!(exchange_test(&mut s, 25, &commit, 8), commit);
        stop(s, 9);
    }
    for scenario in 0..3 {
        let mut s = session();
        begin(&mut s, 1);
        let mut payload = identity(1);
        match scenario {
            0 => {
                observe(&mut s, 1);
                payload.push(1);
            }
            1 => {
                payload[32] ^= 1;
                payload.push(1);
            }
            _ => {}
        }
        send_frame(&mut s.client, 33, &payload).unwrap();
        assert!(receive_response(&mut s.client).is_err());
        assert!(s.server.join().unwrap().is_err());
        assert_eq!(s.installed.load(Ordering::SeqCst), 0);
    }
    resident_scenarios();
    refresh_scenarios();
    retirement_scenarios();
    println!("44 emitted live-protocol scenarios passed");
}

fn resident_scenarios() {
    // The one actual compiled candidate remains pending until exact commit;
    // its derived receipt advances the original profile route.
    {
        let mut s = session();
        begin(&mut s, 1);
        observe(&mut s, 1);
        let bytes = resident(&mut s, 0, 38, true);
        assert_eq!(bytes.len(), 197);
        assert_eq!(&bytes[100..132], &[22; 32]);
        assert_eq!(&bytes[132..164], &[23; 32]);
        assert_eq!(bytes[164], 1);
        assert_eq!(&bytes[165..], &[21; 32]);
        assert_eq!(s.compiled.load(Ordering::SeqCst), 1);
        assert_eq!(s.installed.load(Ordering::SeqCst), 0);
        let commit = resident_commit();
        assert_eq!(exchange_test(&mut s, 25, &commit, 8), commit);
        assert_eq!(s.candidate.load(Ordering::SeqCst), 1);
        assert_eq!(s.compiled.load(Ordering::SeqCst), 1);
        assert_eq!(s.cancelled.load(Ordering::SeqCst), 0);
        let mut message = vec![11; 32];
        message.extend_from_slice(&[8 ^ 11; 32]);
        message.extend_from_slice(&1_u64.to_le_bytes());
        message.extend_from_slice(b"message");
        assert_eq!(exchange_test(&mut s, 14, &message, 15), message);
        stop(s, 11);
    }
    {
        let mut s = session();
        begin(&mut s, 1);
        observe(&mut s, 1);
        resident(&mut s, 0, 38, true);
        let commit = resident_commit();
        assert_eq!(exchange_test(&mut s, 26, &commit, 27), commit);
        assert_eq!(s.cancelled.load(Ordering::SeqCst), 1);
        assert_eq!(s.installed.load(Ordering::SeqCst), 0);
        stop(s, 7);
    }
    // Current misses/diagnostics/resources consume the original request before
    // replying. Late native invalidation cannot launder an error into a miss.
    for (scenario, tag, diagnostic) in [
        (1, 39, ""),
        (2, 40, "source diagnostic"),
        (3, 42, "native resource limit"),
        (4, 41, ""),
        (5, 41, ""),
        (7, 40, "source diagnostic"),
        (8, 42, "native resource limit"),
    ] {
        let mut s = session();
        begin(&mut s, 1);
        observe(&mut s, 1);
        let reply = resident(&mut s, scenario, tag, true);
        assert_eq!(&reply[100..], diagnostic.as_bytes());
        assert_eq!(s.compiled.load(Ordering::SeqCst), 1);
        assert_eq!(s.cancelled.load(Ordering::SeqCst), 1);
        assert_eq!(s.installed.load(Ordering::SeqCst), 0);
        // The reply left no pending candidate or begun request behind.
        begin(&mut s, 2);
        let id = identity(2);
        assert_eq!(exchange_test(&mut s, 31, &id, 32), id);
        assert_eq!(s.cancelled.load(Ordering::SeqCst), 2);
        stop(s, 7);
    }
    // Dirty native baseline before the begin is a miss; a subsequent native
    // event revokes that exact begin and must be obsolete instead.
    for scenario in 0..4 {
        let mut s = session();
        if scenario == 0 {
            s.eligible.store(false, Ordering::SeqCst);
        }
        begin(&mut s, 1);
        observe(&mut s, 1);
        if scenario == 1 || scenario == 2 {
            s.revision.fetch_add(1, Ordering::SeqCst);
            if scenario == 2 {
                s.eligible.store(false, Ordering::SeqCst);
            }
        }
        if scenario == 3 {
            s.revoke_on_eligibility.store(true, Ordering::SeqCst);
        }
        let reply = resident(&mut s, 0, if scenario == 0 { 39 } else { 41 }, false);
        assert_eq!(reply.len(), 100);
        assert_eq!(s.compiled.load(Ordering::SeqCst), 0);
        assert_eq!(s.cancelled.load(Ordering::SeqCst), 1);
        assert_eq!(s.installed.load(Ordering::SeqCst), 0);
        stop(s, 7);
    }
    // An unobserved, foreign, malformed, unowned, repeated, or stale commit
    // request is terminal. None authorizes fallback or installs a candidate.
    for scenario in 0..6 {
        let mut s = session();
        begin(&mut s, 1);
        if scenario != 0 {
            observe(&mut s, 1);
        }
        let mut request = resident_request(1, if scenario == 3 { 6 } else { 0 });
        let mut tag = 37;
        match scenario {
            1 => request[36] ^= 1,
            2 => {
                request.pop();
            }
            4 => {
                resident(&mut s, 1, 39, true);
            }
            5 => {
                resident(&mut s, 0, 38, true);
                request = resident_commit();
                request[0] ^= 1;
                tag = 25;
            }
            _ => {}
        }
        send_frame(&mut s.client, tag, &request).unwrap();
        loop {
            match receive_response(&mut s.client) {
                Ok((24, _)) if scenario == 3 => {}
                Err(_) => break,
                other => panic!("terminal resident transition replied: {other:?}"),
            }
        }
        assert!(s.server.join().unwrap().is_err());
        assert_eq!(s.installed.load(Ordering::SeqCst), 0);
    }
}

fn refresh_scenarios() {
    // Polling only consumes explicitly supplied coalesced intent. It cannot
    // compile, create a pending request, or advance the owner by itself.
    {
        let mut s = session();
        let mut expected = vec![7; 32];
        expected.push(0);
        for _ in 0..3 {
            assert_eq!(exchange_test(&mut s, 43, &[7; 32], 44), expected);
        }
        assert_eq!(s.compiled.load(Ordering::SeqCst), 0);
        assert_eq!(s.revision.load(Ordering::SeqCst), 0);
        assert_eq!(s.cancelled.load(Ordering::SeqCst), 0);
        for _ in 0..3 {
            s.refresh.store(true, Ordering::SeqCst);
        }
        expected[32] = 1;
        assert_eq!(exchange_test(&mut s, 43, &[7; 32], 44), expected);
        expected[32] = 0;
        assert_eq!(exchange_test(&mut s, 43, &[7; 32], 44), expected);
        assert_eq!(s.compiled.load(Ordering::SeqCst), 0);
        assert_eq!(s.installed.load(Ordering::SeqCst), 0);
        stop(s, 7);
    }
    // A committed generation changes the correlation key, never the queued
    // refresh intent. A click during preparation is available after commit.
    {
        let mut s = session();
        begin(&mut s, 1);
        observe(&mut s, 1);
        let commit = prepare(&mut s, 1, false);
        s.refresh.store(true, Ordering::SeqCst);
        assert_eq!(exchange_test(&mut s, 25, &commit, 8), commit);
        let mut expected = vec![9; 32];
        expected.push(1);
        assert_eq!(exchange_test(&mut s, 43, &[9; 32], 44), expected);
        expected[32] = 0;
        assert_eq!(exchange_test(&mut s, 43, &[9; 32], 44), expected);
        stop(s, 9);
    }
    // Polling with a foreign key, malformed payload, begun request, or prepared
    // candidate fails before intent is consumed and never grants a fallback.
    for scenario in 0..4 {
        let mut s = session();
        s.refresh.store(true, Ordering::SeqCst);
        let mut payload = vec![7; 32];
        match scenario {
            0 => payload[0] ^= 1,
            1 => {
                payload.push(0);
            }
            2 => begin(&mut s, 1),
            _ => {
                begin(&mut s, 1);
                observe(&mut s, 1);
                prepare(&mut s, 1, false);
            }
        }
        send_frame(&mut s.client, 43, &payload).unwrap();
        assert!(receive_response(&mut s.client).is_err());
        assert!(s.server.join().unwrap().is_err());
        assert!(s.refresh.load(Ordering::SeqCst));
        assert_eq!(s.installed.load(Ordering::SeqCst), 0);
    }
}

fn retirement_scenarios() {
    // Replacement retirement carries the exact owner's coalesced intent in
    // its final acknowledgement, without compiling or starting another request.
    for refresh in [false, true] {
        let mut s = session();
        s.refresh.store(refresh, Ordering::SeqCst);
        let mut payload = vec![7; 32];
        payload.push(1);
        let mut expected = payload.clone();
        expected[32] = u8::from(refresh);
        assert_eq!(exchange_test(&mut s, 12, &payload, 13), expected);
        assert!(s.server.join().unwrap().is_ok());
        assert!(!s.refresh.load(Ordering::SeqCst));
        assert_eq!(s.compiled.load(Ordering::SeqCst), 0);
        assert_eq!(s.revision.load(Ordering::SeqCst), 0);
        assert_eq!(s.installed.load(Ordering::SeqCst), 0);
    }
    // A foreign key, noncanonical mode, trailing data, begun request, or
    // prepared candidate cannot retire the UI or consume its refresh request.
    for scenario in 0..6 {
        let mut s = session();
        s.refresh.store(true, Ordering::SeqCst);
        let mut payload = vec![7; 32];
        payload.push(1);
        match scenario {
            0 => payload[0] ^= 1,
            1 => payload[32] = 0,
            2 => payload[32] = 2,
            3 => payload.push(0),
            4 => begin(&mut s, 1),
            _ => {
                begin(&mut s, 1);
                observe(&mut s, 1);
                prepare(&mut s, 1, false);
            }
        }
        send_frame(&mut s.client, 12, &payload).unwrap();
        assert!(receive_response(&mut s.client).is_err());
        assert!(s.server.join().unwrap().is_err());
        assert!(s.refresh.load(Ordering::SeqCst));
        assert_eq!(s.installed.load(Ordering::SeqCst), 0);
    }
}
