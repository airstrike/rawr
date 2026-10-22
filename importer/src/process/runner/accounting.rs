//! Request-bound observations, separate from product replies and work progress.

use crate::metrics::{self, accounting::NativeFault};
use crate::protocol::{self, account};

/// At most one report is expected for the current serialized child request.
#[derive(Default)]
pub(super) struct Pending {
    request: Option<account::Request>,
    seen: bool,
}

impl Pending {
    pub(super) fn begin(&mut self, pid: u32, epoch: u64, frame: &protocol::Frame) {
        self.finish(pid, epoch);
        if !account::required(frame.tag()) {
            return;
        }
        match account::Request::from_parts(frame.tag(), frame.payload()) {
            Ok(request) => self.request = Some(request),
            Err(_) => metrics::record_native_fault(pid, epoch, NativeFault::Malformed),
        }
    }

    /// Consume only observational frames. They do not acknowledge a transition,
    /// advance work progress, extend deadlines, or change any admission result.
    pub(super) fn consume(&mut self, pid: u32, epoch: u64, frame: &protocol::Frame) -> bool {
        if frame.tag() != protocol::Tag::Account {
            return false;
        }
        let Some(expected) = self.request else {
            metrics::record_native_fault(pid, epoch, NativeFault::Mismatched);
            return true;
        };
        if self.seen {
            metrics::record_native_fault(pid, epoch, NativeFault::Duplicate);
            return true;
        }
        self.seen = true;
        match account::Observation::decode(frame.payload()) {
            Ok(observation) if observation.request == expected => {
                metrics::record_native(pid, epoch, observation);
            }
            Ok(_) => metrics::record_native_fault(pid, epoch, NativeFault::Mismatched),
            Err(_) => metrics::record_native_fault(pid, epoch, NativeFault::Malformed),
        }
        true
    }

    pub(super) fn finish(&mut self, pid: u32, epoch: u64) {
        if self.request.take().is_some() && !self.seen {
            metrics::record_native_fault(pid, epoch, NativeFault::Missing);
        }
        self.seen = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Sink {
        reports: Mutex<Vec<(u32, u64, account::Observation)>>,
        faults: Mutex<Vec<NativeFault>>,
    }

    impl metrics::Sink for Sink {
        fn node(&self, _: metrics::NodeKind, _: metrics::NodeEvent) {}
        fn process(&self, _: metrics::ProcessKind) {}
        fn milestone(&self, _: metrics::Milestone, _: Option<u64>) {}
        fn runner_owned(&self, _: &metrics::runner::Owned) {}
        fn scope_opened(&self) {}
        fn scope_joined(&self) {}
        #[cfg(feature = "import-baseline")]
        fn profile(&self, _: &metrics::profile::Evidence) {}
        #[cfg(feature = "import-baseline")]
        fn pipeline(&self, _: &metrics::pipeline::Evidence) {}
        fn native(&self, pid: u32, epoch: u64, observation: account::Observation) {
            self.reports.lock().unwrap().push((pid, epoch, observation));
        }
        fn native_fault(&self, _: u32, _: u64, cause: NativeFault) {
            self.faults.lock().unwrap().push(cause);
        }
    }

    fn frame(tag: protocol::Tag, payload: &[u8]) -> protocol::Frame {
        protocol::Frame::new(tag, payload.into()).unwrap()
    }

    fn report(command: &protocol::Frame) -> protocol::Frame {
        let observation = account::Observation {
            request: account::Request::from_parts(command.tag(), command.payload()).unwrap(),
            accounts: [None; 2],
        };
        frame(protocol::Tag::Account, &observation.encode().unwrap())
    }

    #[test]
    fn account_report_preserves_reply_progress_and_owner_identity() {
        let sink = Arc::new(Sink::default());
        let _scope = metrics::Scope::transform(sink.clone()).unwrap();
        let command = frame(protocol::Tag::Prepare, &[3; 80]);
        let mut pending = Pending::default();
        pending.begin(91, 7, &command);
        assert!(!pending.consume(91, 7, &frame(protocol::Tag::WorkProgress, &[0; 51])));
        assert!(pending.consume(91, 7, &report(&command)));
        assert!(!pending.consume(91, 7, &frame(protocol::Tag::Prepared, &[3; 48])));
        pending.finish(91, 7);
        pending.finish(91, 7);
        assert!(sink.faults.lock().unwrap().is_empty());
        let reports = sink.reports.lock().unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!((reports[0].0, reports[0].1), (91, 7));
    }

    #[test]
    fn account_report_faults_cannot_replace_the_original_product_error() {
        let sink = Arc::new(Sink::default());
        let _scope = metrics::Scope::transform(sink.clone()).unwrap();
        let command = frame(protocol::Tag::ResidentCompile, &[5; 96]);
        let mut pending = Pending::default();
        pending.begin(91, 7, &command);
        let original: Result<(), &str> = Err("original allocation refusal");
        pending.finish(91, 7);
        assert_eq!(original, Err("original allocation refusal"));
        assert_eq!(*sink.faults.lock().unwrap(), [NativeFault::Missing]);
        pending.begin(91, 7, &command);
        assert!(pending.consume(91, 7, &frame(protocol::Tag::Account, &[0])));
        pending.finish(91, 7);
        assert_eq!(
            *sink.faults.lock().unwrap(),
            [NativeFault::Missing, NativeFault::Malformed]
        );
    }

    #[test]
    fn account_report_rejects_duplicates_wrong_request_and_unsolicited_frames() {
        let sink = Arc::new(Sink::default());
        let _scope = metrics::Scope::transform(sink.clone()).unwrap();
        let command = frame(protocol::Tag::Prepare, &[3; 80]);
        let mut pending = Pending::default();
        assert!(pending.consume(91, 7, &report(&command)));
        pending.begin(91, 7, &command);
        assert!(pending.consume(91, 7, &report(&frame(protocol::Tag::Prepare, &[4; 80]))));
        pending.finish(91, 7);
        pending.begin(91, 7, &command);
        assert!(pending.consume(91, 7, &report(&command)));
        assert!(pending.consume(91, 7, &report(&command)));
        pending.finish(91, 7);
        assert_eq!(
            *sink.faults.lock().unwrap(),
            [
                NativeFault::Mismatched,
                NativeFault::Mismatched,
                NativeFault::Duplicate
            ]
        );
        assert_eq!(sink.reports.lock().unwrap().len(), 1);
    }
}
