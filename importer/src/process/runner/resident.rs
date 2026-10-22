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
            let response = super::receive_guarded(inner, super::IO_TIMEOUT)?;
            refresh_status(&response, &key)
        })
        .or_else(|error| match error {
            // A window closed between the caller's reap check and this poll;
            // the next reap reports the clean exit.
            Error::WorkPeer {
                status: Some(0), ..
            } => Ok(false),
            error @ Error::WorkPeer { .. } => Err(error),
            error => Err(super::terminal_owner_loss(inner, error)),
        })
    }

    /// The live State as carried bytes for the runner that replaces this one.
    /// `None` when it holds anything but plain script data, or the
    /// window has closed.
    pub fn export_state(&mut self) -> Result<Option<Vec<u8>>, Error> {
        let inner = self.inner.as_mut().ok_or(Error::Consumed)?;
        if !inner.started || !matches!(inner.lifecycle, super::Lifecycle::Run) {
            return Ok(None);
        }
        let key = *inner.key.as_bytes();
        super::guarded(inner, |inner| {
            super::send_guarded(
                inner,
                &protocol::Frame::new(Tag::ExportState, Box::from(key))?,
            )?;
            let response = super::receive_guarded(inner, super::IO_TIMEOUT)?;
            exported_state(&response, &key)
        })
        .or_else(|error| match error {
            Error::WorkPeer {
                status: Some(0), ..
            } => Ok(None),
            error @ Error::WorkPeer { .. } => Err(error),
            error => Err(super::terminal_owner_loss(inner, error)),
        })
    }

    /// Start this admitted, not yet started application from the State a
    /// previous runner exported. `false` when it booted instead: a
    /// script type in the State changed, or it no longer fits.
    pub fn seed_state(&mut self, state: &[u8]) -> Result<bool, Error> {
        let inner = self.inner.as_mut().ok_or(Error::Consumed)?;
        if inner.started || !matches!(inner.lifecycle, super::Lifecycle::Run) {
            return Err(Error::Lifecycle);
        }
        let key = *inner.key.as_bytes();
        let mut payload = Vec::with_capacity(key.len() + state.len());
        payload.extend_from_slice(&key);
        payload.extend_from_slice(state);
        super::guarded(inner, |inner| {
            super::send_guarded(
                inner,
                &protocol::Frame::new(Tag::SeedState, payload.into_boxed_slice())?,
            )?;
            let response = super::receive_guarded(inner, super::IO_TIMEOUT)?;
            flag_status(&response, Tag::StateSeeded, &key)
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
            .manifest
            .native
            .watch())
    }

    /// Compile once inside the admitted live process. A terminal nonprepared
    /// reply consumes the original child request and proves no install occurred.
    pub fn prepare_resident(
        &mut self,
        started: hot::Started,
        revision: crate::supervisor::Revision,
        snapshot: crate::supervisor::snapshot::Bytes,
        freshness: crate::supervisor::snapshot::attempt::Freshness,
        observation: &[u8],
    ) -> Result<resident::Outcome, Error> {
        let inner = self.inner.as_mut().ok_or(Error::Consumed)?;
        if inner.key != started.previous || inner.hot_sequence != started.sequence {
            return Err(Error::Stale);
        }
        if !started.live || !inner.observed {
            super::cancel_started(inner, &started)?;
            return Ok(resident::Outcome::NeedsPreparation);
        }
        let attempt = protocol::resident::Attempt::bind(started.identity(), revision, &snapshot);
        // No request bytes have gone to the child yet; local failures must
        // cancel the retained HotBegin, never leave it blocking later edits.
        let payload = (|| {
            let route = inner.route.ok_or(Error::MissingRoute)?;
            if route.key() != inner.key {
                return Err(Error::Stale);
            }
            if !freshness.matches(&snapshot) {
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
        let response = super::receive_guarded(inner, super::IO_TIMEOUT)?;
        *refresh |= flag_status(&response, Tag::Exited, &key)?;
        Ok(())
    })
}

fn refresh_status(response: &protocol::Frame, key: &[u8; 32]) -> Result<bool, Error> {
    flag_status(response, Tag::RefreshStatus, key)
}

fn exported_state(response: &protocol::Frame, key: &[u8; 32]) -> Result<Option<Vec<u8>>, Error> {
    let payload = response.payload();
    if response.tag() != Tag::StateExported || payload.len() < 33 || payload[..32] != *key {
        return Err(Error::Stale);
    }
    match (payload[32], &payload[33..]) {
        (0, []) => Ok(None),
        (1, state) if !state.is_empty() => Ok(Some(state.to_vec())),
        _ => Err(Error::Stale),
    }
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
    fn exported_state_requires_current_key_tag_and_canonical_payload() {
        let frame = |tag, payload: &[u8]| protocol::Frame::new(tag, Box::from(payload)).unwrap();
        let key = [7; 32];
        let mut none = key.to_vec();
        none.push(0);
        assert_eq!(exported_state(&frame(Tag::StateExported, &none), &key).unwrap(), None);
        let mut some = key.to_vec();
        some.extend_from_slice(&[1, 9, 8]);
        assert_eq!(
            exported_state(&frame(Tag::StateExported, &some), &key).unwrap(),
            Some(vec![9, 8])
        );
        for payload in [&key[..], &[&key[..], &[1]].concat(), &[&key[..], &[0, 5]].concat(), &[&key[..], &[2, 5]].concat()] {
            assert!(exported_state(&frame(Tag::StateExported, payload), &key).is_err());
        }
        assert!(exported_state(&frame(Tag::StateExported, &some), &[8; 32]).is_err());
        assert!(exported_state(&frame(Tag::RefreshStatus, &some), &key).is_err());
    }

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
