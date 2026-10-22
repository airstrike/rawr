//! Transactional application-owner replacement under exact reload authority.

use std::fmt;
use std::sync::Arc;

use rawr::reload::compatibility::{self, Action, Owner, Owners};
use rawr::reload::installation::{self, Replacement};

use crate::{effect, runtime, task, widget};

use super::{Adapter, Draft, Host};

/// One committed application replacement and its optional startup work.
pub struct Outcome {
    evidence: Box<[compatibility::Evidence]>,
    task: iced::Task<runtime::Message>,
    inputs: rawr::reload::scheduler::publication::Retired,
}

/// An application replacement could not transactionally become current.
#[derive(Debug)]
pub enum Error {
    /// Candidate compilation and compatibility planning failed to bind natively.
    Binding(installation::Error),
    /// This installer accepts only the application owner contract.
    Owner,
    /// The candidate has no native application launch.
    Application,
    /// Candidate state, callback, view, theme, or subscription preparation failed.
    Adapter(super::Error),
    /// Reset boot produced a contained runtime diagnostic.
    Candidate(rawr::diagnostic::Report),
    /// Candidate startup work could not be rebound before installation.
    Task(task::Error),
    /// A current off-live failure retained its canonical structured report.
    Diagnostic(rawr::diagnostic::Report),
    /// Exact source or manifest contents changed after candidate compilation.
    Source(rawr::reload::source::Error),
    /// The exact host identity, state stamp, or generation proof became stale.
    Stale,
}

impl Host {
    /// Compile-proven candidate installation owned synchronously by this host.
    ///
    /// No prepared binding or raw commit capability crosses this boundary.
    pub fn install(
        &mut self,
        gate: &rawr::reload::scheduler::Gate,
        publication: &rawr::reload::scheduler::publication::Slot,
        candidate: rawr::reload::scheduler::Candidate<(
            rawr::reload::Candidate,
            rawr::reload::scheduler::publication::Staged,
        )>,
        engine: &rawr::Shared<rawr::Engine>,
        runtime: rawr::host::runtime::Registry,
    ) -> Result<rawr::reload::scheduler::Admission<(), Outcome>, Error> {
        self.install_with_timing(gate, publication, candidate, engine, runtime, None)
    }

    pub(super) fn install_observed(
        &mut self,
        gate: &rawr::reload::scheduler::Gate,
        publication: &rawr::reload::scheduler::publication::Slot,
        candidate: rawr::reload::scheduler::Candidate<(
            rawr::reload::Candidate,
            rawr::reload::scheduler::publication::Staged,
        )>,
        engine: &rawr::Shared<rawr::Engine>,
        runtime: rawr::host::runtime::Registry,
        timing: Option<&rawr::timing::Session>,
    ) -> Result<rawr::reload::scheduler::Admission<(), Outcome>, Error> {
        self.install_with_timing(gate, publication, candidate, engine, runtime, timing)
    }

    fn install_with_timing(
        &mut self,
        gate: &rawr::reload::scheduler::Gate,
        publication: &rawr::reload::scheduler::publication::Slot,
        candidate: rawr::reload::scheduler::Candidate<(
            rawr::reload::Candidate,
            rawr::reload::scheduler::publication::Staged,
        )>,
        engine: &rawr::Shared<rawr::Engine>,
        runtime: rawr::host::runtime::Registry,
        timing: Option<&rawr::timing::Session>,
    ) -> Result<rawr::reload::scheduler::Admission<(), Outcome>, Error> {
        let source = Some(
            candidate
                .value()
                .0
                .source()
                .entry_source()
                .diagnostic()
                .clone(),
        );
        let validate = || {
            let candidate = candidate.map(|(candidate, inputs)| {
                (
                    compatibility::Plan::compare(
                        self.slot.generation(),
                        candidate,
                        Owners::one(Owner::Application),
                    ),
                    inputs,
                )
            });
            let candidate = candidate.map_result(|(plan, inputs)| {
                plan.bind(engine, runtime)
                    .map(|bound| (bound, inputs))
                    .map_err(Error::Binding)
            })?;
            let candidate = candidate.map_result(|(replacement, inputs)| {
                prepare(self, replacement).map(|prepared| (prepared, inputs))
            })?;
            candidate.map_result(|(prepared, inputs)| {
                inputs
                    .revalidate()
                    .map(|validated| (prepared, validated))
                    .map_err(Error::Source)
            })
        };
        let candidate = match timing {
            Some(timing) => {
                timing.observe_result(rawr::timing::Phase::CandidateValidation, validate)
            }
            None => validate(),
        };
        let candidate = match candidate {
            Ok(candidate) => candidate,
            Err(error) => return settle_failure(gate, error, source),
        };
        let admit = || gate.admit_slot_and_publish(candidate, &mut self.slot, publication);
        let admission = match timing {
            Some(timing) => {
                timing.observe_outcome_unwind(rawr::timing::Phase::Installation, || {
                    let admission = admit();
                    let outcome = match &admission {
                        rawr::reload::scheduler::Admission::Current(Ok(_)) => {
                            rawr::timing::Outcome::Success
                        }
                        rawr::reload::scheduler::Admission::Current(Err(_)) => {
                            rawr::timing::Outcome::Failure
                        }
                        rawr::reload::scheduler::Admission::Obsolete(_) => {
                            rawr::timing::Outcome::Obsolete
                        }
                    };
                    (admission, outcome)
                })
            }
            None => admit(),
        };
        match admission {
            rawr::reload::scheduler::Admission::Current(Ok((evidence, retired, inputs))) => {
                let task = {
                    let mut mutation = self.slot.mutation();
                    let (adapter, context) = mutation.parts();
                    adapter
                        .authority(&context)
                        .map_or_else(iced::Task::none, |authority| {
                            adapter.activate_prepared(&context, &authority)
                        })
                };
                match timing {
                    Some(timing) => {
                        timing.observe_unwind(rawr::timing::Phase::EffectRetirement, || {
                            drop(retired.release())
                        });
                    }
                    None => drop(retired.release()),
                }
                Ok(rawr::reload::scheduler::Admission::Current(Outcome {
                    evidence,
                    task,
                    inputs,
                }))
            }
            rawr::reload::scheduler::Admission::Current(Err(rejected)) => {
                drop(rejected.release());
                Err(Error::Stale)
            }
            rawr::reload::scheduler::Admission::Obsolete(candidate) => {
                drop(candidate);
                Ok(rawr::reload::scheduler::Admission::Obsolete(()))
            }
        }
    }
}

fn settle_failure(
    gate: &rawr::reload::scheduler::Gate,
    error: rawr::reload::scheduler::Candidate<Error>,
    source: Option<rawr::diagnostic::Source>,
) -> Result<rawr::reload::scheduler::Admission<(), Outcome>, Error> {
    match gate.retain_current(error) {
        rawr::reload::scheduler::Admission::Current(error) => {
            let report = error.value().report(source);
            drop(error);
            Err(Error::Diagnostic(report))
        }
        rawr::reload::scheduler::Admission::Obsolete(error) => {
            drop(error);
            Ok(rawr::reload::scheduler::Admission::Obsolete(()))
        }
    }
}

fn prepare(
    host: &Host,
    replacement: Replacement,
) -> Result<installation::current::Prepared<Adapter>, Error> {
    if replacement.evidence().len() != 1 || replacement.owner(&Owner::Application).is_none() {
        return Err(Error::Owner);
    }
    let action = replacement
        .owner(&Owner::Application)
        .map(compatibility::Evidence::action)
        .ok_or(Error::Owner)?;
    let source = Some(
        replacement
            .generation()
            .source()
            .entry_source()
            .diagnostic()
            .clone(),
    );
    let preparing = replacement.prepare(&host.slot).map_err(|_| Error::Stale)?;
    let adapter = {
        let preparation = preparing.preparation();
        let previous = preparing.state();
        let catalog = preparation.catalog();
        let launch = catalog.application().ok_or(Error::Application)?;
        let mut adapter = match action {
            Action::Retain => retain(previous, &launch, source, host.timing())?,
            Action::Reset => {
                let mut adapter =
                    Draft::boot(&launch, source, host.timing().cloned()).map_err(Error::Adapter)?;
                if let Some(entry) = adapter.diagnostics.first() {
                    return Err(Error::Candidate(entry.report().clone()));
                }
                adapter.timing = None;
                adapter
            }
        };
        let startup = std::mem::take(&mut adapter.startup);
        let task = startup.prepare_application(&launch).map_err(Error::Task)?;
        let mut adapter = adapter.install(launch.generation());
        adapter.prepared = Some(task);
        adapter
    };
    Ok(preparing.seal(adapter))
}

impl Outcome {
    /// Return the exact compatibility action and reasons that were committed.
    #[must_use = "compatibility evidence explains the committed replacement"]
    pub const fn evidence(&self) -> &[compatibility::Evidence] {
        &self.evidence
    }

    /// Consume the committed replacement into startup work for iced.
    #[must_use = "startup work must be returned to iced"]
    pub fn into_task(self) -> iced::Task<runtime::Message> {
        let (task, inputs) = self.into_parts();
        inputs.release();
        task
    }

    /// Consume the admitted replacement into startup work and retired inputs.
    #[must_use = "startup work and retired inputs must be reconciled"]
    pub fn into_parts(
        self,
    ) -> (
        iced::Task<runtime::Message>,
        rawr::reload::scheduler::publication::Retired,
    ) {
        (self.task, self.inputs)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binding(error) => error.fmt(formatter),
            Self::Owner => formatter.write_str("replacement does not prove the application owner"),
            Self::Application => formatter.write_str("replacement has no application launch"),
            Self::Adapter(error) => error.fmt(formatter),
            Self::Candidate(report) => report.fmt(formatter),
            Self::Task(error) => error.fmt(formatter),
            Self::Diagnostic(report) => report.fmt(formatter),
            Self::Source(error) => error.fmt(formatter),
            Self::Stale => formatter
                .write_str("replacement no longer matches the exact application host state"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Binding(error) => Some(error),
            Self::Adapter(error) => Some(error),
            Self::Task(error) => Some(error),
            Self::Source(error) => Some(error),
            Self::Owner
            | Self::Application
            | Self::Candidate(_)
            | Self::Diagnostic(_)
            | Self::Stale => None,
        }
    }
}

impl Error {
    pub(crate) fn report(
        &self,
        source: Option<rawr::diagnostic::Source>,
    ) -> rawr::diagnostic::Report {
        match self {
            Self::Candidate(report) | Self::Diagnostic(report) => report.clone(),
            Self::Adapter(super::Error::ExactSource { report, .. }) => report.clone(),
            Self::Binding(_) => crate::reload::report(
                rawr::diagnostic::code::FRAMEWORK_RUNTIME_BINDING,
                self,
                source,
            ),
            Self::Source(_) => {
                crate::reload::report(rawr::diagnostic::code::ENVIRONMENT_DIAGNOSTIC, self, source)
            }
            Self::Owner | Self::Application | Self::Adapter(_) | Self::Task(_) | Self::Stale => {
                crate::reload::report(rawr::diagnostic::code::FRAMEWORK_NATIVE, self, source)
            }
        }
    }
}

pub(crate) fn retain(
    adapter: &Adapter,
    launch: &rawr::reload::installation::Application<'_>,
    source: Option<rawr::diagnostic::Source>,
    timing: Option<&rawr::timing::Session>,
) -> Result<Draft, Error> {
    launch
        .validate_state(&adapter.state)
        .map_err(super::Error::Boot)
        .map_err(Error::Adapter)?;
    let state = adapter.state.clone();
    let display = match timing {
        Some(timing) => timing
            .observe_result(rawr::timing::Phase::ViewEvaluation, || {
                super::evaluate_view(launch, &state)
            })
            .map_err(Error::Adapter)?,
        None => super::evaluate_view(launch, &state).map_err(Error::Adapter)?,
    };
    let title = super::evaluate_title(launch, &state)
        .map_err(|error| match launch.title() {
            Some(title) => error.with_callback(launch, title.callback()),
            None => error,
        })
        .map_err(Error::Adapter)?
        .unwrap_or_else(|| "Rawr application".to_owned());
    let theme = super::evaluate_theme(launch, &state)
        .map_err(Error::Adapter)?
        .unwrap_or_default();
    let subscription = super::evaluate_subscription(launch, &state).map_err(Error::Adapter)?;
    let lifecycle = Arc::new(widget::diagnostic::Sink::new(
        adapter.instance,
        launch.generation(),
    ));
    Ok(Draft {
        instance: adapter.instance,
        lifecycle,
        state,
        display: display.clone(),
        last_good_output: Some(display),
        title: title.clone(),
        last_good_title: Some(title),
        theme: theme.clone(),
        last_good_theme: Some(theme),
        diagnostics: Vec::new(),
        startup: task::Plan::none(),
        tasks: effect::Tasks::default(),
        subscription,
        centered: launch.centered(),
        source,
        timing: None,
    })
}
