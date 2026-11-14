//! Authenticated attempt-first compilation against this process's native owner.

use super::{Candidate, Error, ProgressState, WorkOperation};
use crate::process::{hot, resident};
use crate::protocol::{self, Tag};

impl<B> Candidate<super::Admitted, B> {
    /// Drain a coalesced explicit UI refresh request on the idle live channel.
    /// This performs no source validation or background Cargo preparation.
    pub fn take_refresh(&mut self) -> Result<bool, Error> {
        let inner = self.inner.as_mut().ok_or(Error::Consumed)?;
        if !inner.started || !inner.observed {
            return Ok(false);
        }
        let key = *inner.key.as_bytes();
        super::guarded(inner, |inner| {
            super::send_guarded(
                inner,
                &protocol::Frame::new(Tag::RefreshPoll, Box::from(key))?,
            )?;
            let response = super::receive_reply(inner, None)?;
            refresh_status(&response, &key)
        })
        .map_err(|error| match error {
            // The program ended as it was polled: the next reap says how.
            error @ Error::WorkPeer { .. } => error,
            error => super::terminal_owner_loss(inner, error),
        })
    }

    /// Native watch obligations remain those of the owned artifact, even when
    /// its Rawr snapshot changes. The caller joins these with fresh Rawr inputs.
    pub fn native_watch(
        &self,
    ) -> Result<impl Iterator<Item = crate::input::proof::Watch> + '_, Error> {
        Ok(self
            .inner
            .as_ref()
            .ok_or(Error::Consumed)?
            .manifest()
            .native
            .watch())
    }

    /// The identity of this runner's native input proof: two runners whose
    /// proofs share it watch the same native inputs ([`Self::native_watch`]).
    pub fn native_fingerprint(&self) -> Result<crate::input::fingerprint::Fingerprint, Error> {
        Ok(self
            .inner
            .as_ref()
            .ok_or(Error::Consumed)?
            .manifest()
            .native
            .fingerprint())
    }

    /// Why `request` needs another runner than this one, in one line,
    /// naming no path under `outputs`, the directories builds write;
    /// `None` when this runner serves it.
    pub fn native_change(
        &self,
        request: &crate::bridge::GenerationRequestKey<B>,
        outputs: &[std::path::PathBuf],
    ) -> Result<Option<String>, Error> {
        let manifest = self.inner.as_ref().ok_or(Error::Consumed)?.manifest();
        Ok(request.change_from(manifest, outputs))
    }

    /// Compile once inside the admitted live process. A terminal nonprepared
    /// reply consumes the original child request and proves no install occurred.
    pub fn prepare_resident(
        &mut self,
        started: hot::Started,
        revision: crate::supervisor::Revision,
        snapshot: &crate::supervisor::snapshot::Bytes,
        freshness: crate::supervisor::snapshot::attempt::Freshness,
        observation: &[u8],
    ) -> Result<resident::Outcome, Error> {
        let inner = self.inner.as_mut().ok_or(Error::Consumed)?;
        if inner.key != started.previous || inner.hot_sequence != started.sequence {
            return Err(Error::Stale);
        }
        if !started.live || !inner.observed {
            super::cancel_started(inner, &started)?;
            return Ok(resident::Outcome::Unstarted);
        }
        let attempt = protocol::resident::Attempt::bind(started.identity(), revision, snapshot);
        // No request bytes have gone to the child yet; local failures must
        // cancel the retained HotBegin, never leave it blocking later edits.
        let payload = (|| {
            let route = inner.route.ok_or(Error::MissingRoute)?;
            if route.key() != inner.key {
                return Err(Error::Stale);
            }
            if !freshness.matches(snapshot) {
                return Err(Error::Snapshot(
                    crate::supervisor::snapshot::Error::Mismatch,
                ));
            }
            let observation = super::observation_payload(&started.identity(), observation)?;
            Ok((attempt.encode(snapshot)?, observation, route))
        })();
        let (payload, observation, route) = match payload {
            Ok(payload) => payload,
            Err(failure) => {
                return match super::cancel_started(inner, &started) {
                    Ok(()) => Err(failure),
                    Err(cleanup) => Err(Error::HotCleanup {
                        failure: Box::new(failure),
                        cleanup: Box::new(cleanup),
                    }),
                }
            }
        };
        let identity = started.identity();
        let observed = super::exchange(
            inner,
            Tag::HotObserve,
            observation,
            Tag::HotObserved,
            &identity,
        );
        if let Err(error) = observed {
            return Err(super::terminal_owner_loss(inner, error));
        }
        let response = super::work_request(
            inner,
            Tag::ResidentCompile,
            payload,
            ProgressState::new(
                *started.previous.as_bytes(),
                started.nonce,
                WorkOperation::Compile,
            ),
        )
        .map_err(|error| super::terminal_owner_loss(inner, error))?;
        if response.tag() == Tag::ResidentPrepared {
            let evidence = attempt
                .accept(response.payload(), &inner.resident_carrier)
                .map_err(|error| super::terminal_owner_loss(inner, error.into()))?;
            let next = crate::supervisor::GenerationKey::resident(&evidence);
            let route = route.advance(next);
            let prepared = resident::Prepared {
                previous: inner.key,
                next,
                nonce: started.nonce,
                route,
                freshness,
            };
            // The child owns pending state now. A failed after-check must abort
            // exactly that candidate; uncertainty retires the owner.
            if let Err(failure) = prepared.revalidate() {
                return match abort(inner, &prepared) {
                    Ok(()) => Err(failure.into()),
                    Err(cleanup) => Err(Error::HotCleanup {
                        failure: Box::new(failure.into()),
                        cleanup: Box::new(cleanup),
                    }),
                };
            }
            return Ok(resident::Outcome::Prepared(prepared));
        }
        let reason = attempt
            .accept_status(response.payload())
            .map_err(|error| super::terminal_owner_loss(inner, error.into()))?;
        match response.tag() {
            Tag::ResidentMiss if reason.is_empty() => Ok(resident::Outcome::NeedsPreparation),
            Tag::ResidentObsolete if reason.is_empty() => Ok(resident::Outcome::Obsolete),
            Tag::ResidentDiagnostic => Ok(resident::Outcome::Source(reason.into())),
            Tag::ResidentResource => Ok(resident::Outcome::Resource(reason.into())),
            Tag::ResidentRefused => Ok(resident::Outcome::Refused(reason.into())),
            _ => Err(super::terminal_owner_loss(inner, Error::Stale)),
        }
    }

    /// Preserve the native closure's existing Live/LastGood ownership. No new
    /// store node or pin grants authority to this in-process Rawr generation.
    pub fn commit_resident(
        &mut self,
        prepared: resident::Prepared,
    ) -> Result<crate::supervisor::GenerationKey, Error> {
        let inner = self.inner.as_mut().ok_or(Error::Consumed)?;
        if inner.key != prepared.previous {
            return Err(Error::Stale);
        }
        if let Err(failure) = prepared.revalidate() {
            return match abort(inner, &prepared) {
                Ok(()) => Err(failure.into()),
                Err(cleanup) => Err(Error::HotCleanup {
                    failure: Box::new(failure.into()),
                    cleanup: Box::new(cleanup),
                }),
            };
        }
        if let Err(error) = super::hot_commit_exchange(inner, &prepared.identity()) {
            return Err(if error.is_hot_consumed_rejection() {
                error
            } else {
                super::terminal_owner_loss(inner, error)
            });
        }
        // A program blocked reading the terminal draws again with the
        // generation it just got (D534.4), as after a prepared commit.
        inner.child.wake();
        inner.key = prepared.next;
        inner.route = Some(prepared.route);
        inner.sequence = crate::profile::message::Sequence::first();
        Ok(prepared.next)
    }

    pub fn abort_resident(&mut self, prepared: resident::Prepared) -> Result<(), Error> {
        let inner = self.inner.as_mut().ok_or(Error::Consumed)?;
        if inner.key != prepared.previous {
            return Err(Error::Stale);
        }
        abort(inner, &prepared)
    }
}

fn abort<B>(inner: &mut super::Inner<B>, prepared: &resident::Prepared) -> Result<(), Error> {
    let identity = prepared.identity();
    super::exchange(
        inner,
        Tag::HotAbort,
        identity.to_vec(),
        Tag::HotAborted,
        &identity,
    )
    .map_err(|error| super::terminal_owner_loss(inner, error))
}

pub(super) fn retire<B>(inner: &mut super::Inner<B>, refresh: &mut bool) -> Result<(), Error> {
    let key = *inner.key.as_bytes();
    super::guarded(inner, |inner| {
        let mut payload = [0_u8; 33];
        payload[..32].copy_from_slice(&key);
        payload[32] = 1;
        super::send_guarded(
            inner,
            &protocol::Frame::new(Tag::Shutdown, Box::from(payload))?,
        )?;
        let response = super::receive_reply(inner, None)?;
        *refresh |= flag_status(&response, Tag::Exited, &key)?;
        Ok(())
    })
}

fn refresh_status(response: &protocol::Frame, key: &[u8; 32]) -> Result<bool, Error> {
    flag_status(response, Tag::RefreshStatus, key)
}

fn flag_status(response: &protocol::Frame, tag: Tag, key: &[u8; 32]) -> Result<bool, Error> {
    if response.tag() != tag || response.payload().len() != 33 || response.payload()[..32] != *key {
        return Err(Error::Stale);
    }
    match response.payload()[32] {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Error::Stale),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_status_requires_current_key_tag_and_canonical_flag() {
        let mut payload = [7; 33];
        for flag in [0, 1, 2] {
            payload[32] = flag;
            let frame = protocol::Frame::new(Tag::RefreshStatus, Box::from(payload)).unwrap();
            let result = refresh_status(&frame, &[7; 32]);
            assert_eq!(result.is_ok(), flag < 2);
            if flag < 2 {
                assert_eq!(result.unwrap(), flag == 1);
            }
            assert!(refresh_status(&frame, &[8; 32]).is_err());
        }
        for length in [0, 31, 32] {
            let frame = protocol::Frame::new(Tag::RefreshStatus, payload[..length].into()).unwrap();
            assert!(refresh_status(&frame, &[7; 32]).is_err());
        }
        let frame = protocol::Frame::new(Tag::ProfileReply, Box::from(payload)).unwrap();
        assert!(refresh_status(&frame, &[7; 32]).is_err());
    }

    #[test]
    fn retirement_status_requires_exact_key_tag_length_and_flag() {
        for flag in [0, 1, 2] {
            let mut payload = vec![7; 32];
            payload.push(flag);
            let frame = protocol::Frame::new(Tag::Exited, payload.clone().into()).unwrap();
            assert_eq!(flag_status(&frame, Tag::Exited, &[7; 32]).is_ok(), flag < 2);
            if flag < 2 {
                assert_eq!(
                    flag_status(&frame, Tag::Exited, &[7; 32]).unwrap(),
                    flag == 1
                );
            }
            assert!(flag_status(&frame, Tag::Exited, &[8; 32]).is_err());
            for tag in [Tag::RefreshStatus, Tag::Shutdown] {
                let frame = protocol::Frame::new(tag, payload.clone().into()).unwrap();
                assert!(flag_status(&frame, Tag::Exited, &[7; 32]).is_err());
            }
        }
        for length in [0, 31, 32, 34] {
            let frame = protocol::Frame::new(Tag::Exited, vec![7; length].into()).unwrap();
            assert!(flag_status(&frame, Tag::Exited, &[7; 32]).is_err());
        }
    }

    #[test]
    #[cfg(unix)]
    fn retirement_read_shutdown_unblocks_reader_and_preserves_final_ack() {
        use std::io::{Read as _, Write as _};
        use std::net::Shutdown;
        use std::os::unix::net::UnixStream;

        let (mut parent, mut worker) = UnixStream::pair().unwrap();
        let control = worker.try_clone().unwrap();
        let (ready, waiting) = std::sync::mpsc::sync_channel(0);
        let worker = std::thread::spawn(move || {
            ready.send(()).unwrap();
            let mut byte = [0];
            assert_eq!(worker.read(&mut byte).unwrap(), 0);
            // Main-thread UI exit must not cut off this final keyed reply.
            let mut acknowledgement = [7; 33];
            acknowledgement[32] = 1;
            worker.write_all(&acknowledgement).unwrap();
        });
        waiting.recv().unwrap();
        control.shutdown(Shutdown::Read).unwrap();
        let mut acknowledgement = [0; 33];
        parent.read_exact(&mut acknowledgement).unwrap();
        assert_eq!(&acknowledgement[..32], &[7; 32]);
        assert_eq!(acknowledgement[32], 1);
        worker.join().unwrap();
    }
}
