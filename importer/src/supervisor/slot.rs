//! Move-only whole-process last-good admission.

/// The supervisor's current admitted runner, separate from in-process reload.
pub struct Slot<B> {
    current: Option<crate::process::Candidate<crate::process::Admitted, B>>,
    newest_miss: Option<super::Revision>,
}

/// Result of trying to install a cold candidate against the latest miss gate.
pub enum Installation<B> {
    Installed(Option<crate::process::Candidate<crate::process::Admitted, B>>),
    Obsolete(crate::process::Candidate<crate::process::Admitted, B>),
}

impl<B> Slot<B> {
    pub fn take_refresh(&mut self) -> Result<bool, crate::process::RunnerError> {
        let result = self
            .current
            .as_mut()
            .ok_or(crate::process::RunnerError::NoCurrent)?
            .take_refresh();
        if result
            .as_ref()
            .is_err_and(crate::process::RunnerError::is_owner_lost)
        {
            drop(self.current.take());
        }
        result
    }

    /// Start `candidate`, admitted and not yet started, from the current
    /// runner's State. Whether it was seeded: anything but plain script
    /// data, a changed script type or a lost runner leaves its booted State.
    /// A State that holds anything but plain script data is sent as no State,
    /// so the candidate can say which of its types did not carry over.
    pub fn carry_state(
        &mut self,
        candidate: &mut crate::process::Candidate<crate::process::Admitted, B>,
    ) -> bool {
        let Some(current) = self.current.as_mut() else {
            return false;
        };
        let state = match current.export_state() {
            Ok(Some(state)) => state,
            Ok(None) => Vec::new(),
            Err(error) => {
                if error.is_owner_lost() {
                    drop(self.current.take());
                }
                return false;
            }
        };
        candidate.seed_state(&state).unwrap_or(false)
    }

    /// Borrow watch obligations from the actual owned native artifact.
    pub fn native_watch(
        &self,
    ) -> Result<impl Iterator<Item = crate::input::proof::Watch> + '_, crate::process::RunnerError>
    {
        self.current
            .as_ref()
            .ok_or(crate::process::RunnerError::NoCurrent)?
            .native_watch()
    }

    pub fn prepare_resident(
        &mut self,
        started: crate::process::hot::Started,
        revision: super::Revision,
        snapshot: super::snapshot::Bytes,
        freshness: super::snapshot::attempt::Freshness,
        observation: &[u8],
    ) -> Result<crate::process::resident::Outcome, crate::process::RunnerError> {
        let result = self
            .current
            .as_mut()
            .ok_or(crate::process::RunnerError::NoCurrent)?
            .prepare_resident(started, revision, snapshot, freshness, observation);
        if result
            .as_ref()
            .is_err_and(crate::process::RunnerError::is_owner_lost)
        {
            drop(self.current.take());
        }
        result
    }

    pub fn commit_resident(
        &mut self,
        prepared: crate::process::resident::Prepared,
    ) -> Result<super::GenerationKey, crate::process::RunnerError> {
        let result = self
            .current
            .as_mut()
            .ok_or(crate::process::RunnerError::NoCurrent)?
            .commit_resident(prepared);
        if result
            .as_ref()
            .is_err_and(crate::process::RunnerError::is_owner_lost)
        {
            drop(self.current.take());
        }
        result
    }

    pub fn abort_resident(
        &mut self,
        prepared: crate::process::resident::Prepared,
    ) -> Result<(), crate::process::RunnerError> {
        let result = self
            .current
            .as_mut()
            .ok_or(crate::process::RunnerError::NoCurrent)?
            .abort_resident(prepared);
        if result
            .as_ref()
            .is_err_and(crate::process::RunnerError::is_owner_lost)
        {
            drop(self.current.take());
        }
        result
    }

    /// Create an empty process route before cold bootstrap.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            current: None,
            newest_miss: None,
        }
    }

    /// Move one fully admitted candidate into the live route and return the
    /// exact retired process owner without running retirement under a lock.
    pub fn install(
        &mut self,
        candidate: crate::process::Candidate<crate::process::Admitted, B>,
    ) -> Option<crate::process::Candidate<crate::process::Admitted, B>> {
        self.newest_miss = None;
        self.current.replace(candidate)
    }

    /// Install only a candidate built from the newest coalesced miss. An
    /// obsolete candidate is returned without disturbing the last-good route.
    pub fn install_latest(
        &mut self,
        candidate: crate::process::Candidate<crate::process::Admitted, B>,
    ) -> Installation<B> {
        if self
            .newest_miss
            .is_some_and(|revision| revision != candidate.revision())
        {
            return Installation::Obsolete(candidate);
        }
        self.newest_miss = None;
        Installation::Installed(self.current.replace(candidate))
    }

    /// Borrow the complete identity of the current route.
    #[must_use]
    pub fn key(&self) -> Option<super::GenerationKey> {
        self.current.as_ref().map(crate::process::Candidate::key)
    }

    /// Admit a same-surface request inside the current runner or preserve the
    /// complete inputs for one cold replacement when no compatible runner exists.
    pub fn compile(
        &mut self,
        request: crate::bridge::GenerationRequestKey<B>,
        snapshot: super::snapshot::Bytes,
    ) -> Result<crate::process::hot::Outcome<B>, crate::process::RunnerError>
    where
        B: crate::surface::Runner,
    {
        match self.prepare_compile(request, snapshot)? {
            crate::process::hot::Preparation::Prepared(prepared) => self
                .commit_compile(prepared)
                .map(crate::process::hot::Outcome::Hit),
            crate::process::hot::Preparation::Miss(miss) => {
                Ok(crate::process::hot::Outcome::Miss(miss))
            }
        }
    }

    /// Begin on the current live owner unless a cold replacement is already pending.
    pub fn begin_compile(
        &mut self,
    ) -> Result<Option<crate::process::hot::Started>, crate::process::RunnerError> {
        if self.newest_miss.is_some() {
            return Ok(None);
        }
        let result = self
            .current
            .as_mut()
            .map(|current| current.begin_compile())
            .transpose();
        if result.as_ref().is_err_and(|error| error.is_owner_lost()) {
            drop(self.current.take());
        }
        result
    }

    /// Cancel the original early request, if this lane had a reusable owner.
    pub fn cancel_compile(
        &mut self,
        started: Option<crate::process::hot::Started>,
    ) -> Result<(), crate::process::RunnerError> {
        let Some(started) = started else {
            return Ok(());
        };
        let result = self
            .current
            .as_mut()
            .ok_or(crate::process::RunnerError::NoCurrent)?
            .cancel_compile(started);
        if result.as_ref().is_err_and(|error| error.is_owner_lost()) {
            drop(self.current.take());
        }
        result
    }

    /// Prepare complete inputs using a newly captured request.
    pub fn prepare_compile(
        &mut self,
        request: crate::bridge::GenerationRequestKey<B>,
        snapshot: super::snapshot::Bytes,
    ) -> Result<crate::process::hot::Preparation<B>, crate::process::RunnerError>
    where
        B: crate::surface::Runner,
    {
        let started = self.begin_compile()?;
        self.prepare_started_compile(started, request, snapshot, None)
    }

    /// Bind prepared inputs to the child request captured before parent work.
    pub fn prepare_started_compile(
        &mut self,
        started: Option<crate::process::hot::Started>,
        request: crate::bridge::GenerationRequestKey<B>,
        snapshot: super::snapshot::Bytes,
        observation: Option<&[u8]>,
    ) -> Result<crate::process::hot::Preparation<B>, crate::process::RunnerError>
    where
        B: crate::surface::Runner,
    {
        let revision = request.revision();
        let outcome = match started {
            Some(started) => {
                let result = self
                    .current
                    .as_mut()
                    .ok_or(crate::process::RunnerError::NoCurrent)?
                    .prepare_started_compile(started, request, snapshot, observation);
                match result {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        if error.is_owner_lost() {
                            drop(self.current.take());
                        }
                        return Err(error);
                    }
                }
            }
            None => crate::process::hot::Preparation::Miss(crate::process::hot::Miss::new(
                request, snapshot,
            )),
        };
        if matches!(outcome, crate::process::hot::Preparation::Miss(_)) {
            self.newest_miss = self.newest_miss.max(Some(revision));
        }
        Ok(outcome)
    }

    /// Commit one externally admitted same-process preparation.
    pub fn commit_compile(
        &mut self,
        prepared: crate::process::hot::Prepared<B>,
    ) -> Result<super::GenerationKey, crate::process::RunnerError> {
        let result = self
            .current
            .as_mut()
            .ok_or(crate::process::RunnerError::NoCurrent)?
            .commit_compile(prepared);
        if result
            .as_ref()
            .is_err_and(crate::process::RunnerError::is_owner_lost)
        {
            drop(self.current.take());
        }
        result
    }

    /// Abort one externally revoked same-process preparation without changing
    /// the current generation.
    pub fn abort_compile(
        &mut self,
        prepared: crate::process::hot::Prepared<B>,
    ) -> Result<(), crate::process::RunnerError> {
        let result = self
            .current
            .as_mut()
            .ok_or(crate::process::RunnerError::NoCurrent)?
            .abort_compile(prepared);
        if result
            .as_ref()
            .is_err_and(crate::process::RunnerError::is_owner_lost)
        {
            drop(self.current.take());
        }
        result
    }

    /// Send one bounded message through the current admitted profile route.
    pub fn message<M: crate::profile::message::Message>(
        &mut self,
        message: M,
    ) -> Result<crate::profile::message::Envelope<M>, crate::process::RunnerError> {
        self.current
            .as_mut()
            .ok_or(crate::process::RunnerError::NoCurrent)?
            .message(message)
    }

    /// Start the current admitted live lifecycle.
    pub fn start(&mut self) -> Result<(), crate::process::RunnerError> {
        self.current
            .as_mut()
            .ok_or(crate::process::RunnerError::NoCurrent)?
            .start()
    }

    /// Atomically replace the durable Live and LastGood store roles with the
    /// current admitted runner's exact transitive closure.
    pub fn pin_current_live_last_good(&self) -> Result<(), crate::node::Error> {
        self.current
            .as_ref()
            .ok_or(crate::node::Error::State(
                "runner slot has no current generation to pin",
            ))?
            .pin_live_last_good()
    }

    /// Execute the current admitted one-shot snapshot lifecycle.
    pub fn snapshot(&mut self) -> Result<Box<[u8]>, crate::process::RunnerError> {
        let result = self
            .current
            .as_mut()
            .ok_or(crate::process::RunnerError::NoCurrent)?
            .snapshot();
        if result
            .as_ref()
            .is_err_and(crate::process::RunnerError::is_owner_lost)
        {
            drop(self.current.take());
        }
        result
    }

    /// Forward one bounded rendered diagnostic to the current live owner.
    pub fn report(&mut self, report: &str) -> Result<(), crate::process::RunnerError> {
        self.current
            .as_mut()
            .ok_or(crate::process::RunnerError::NoCurrent)?
            .report(report)
    }

    /// Reap and retire the current child after its native window closes.
    pub fn reap_if_exited(&mut self) -> Result<bool, crate::process::RunnerError> {
        let exited = match self.current.as_mut() {
            Some(current) => current.reap_if_exited()?,
            None => return Ok(true),
        };
        if exited {
            drop(self.current.take());
        }
        Ok(exited)
    }

    /// Validate a queued message against the current exact profile route.
    pub fn validate_message<M>(
        &self,
        envelope: &crate::profile::message::Envelope<M>,
    ) -> Result<(), crate::process::RunnerError> {
        self.current
            .as_ref()
            .ok_or(crate::process::RunnerError::NoCurrent)?
            .validate_message(envelope)
    }

    /// Remove the current process for explicit shutdown.
    pub fn take(&mut self) -> Option<crate::process::Candidate<crate::process::Admitted, B>> {
        self.current.take()
    }
}

impl<B> Default for Slot<B> {
    fn default() -> Self {
        Self::new()
    }
}
