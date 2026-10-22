//! Transactional replacement for one application/preview host selection.

use rawr::reload::compatibility::{self, Action, Owner, Owners};
use rawr::reload::installation::{self, Replacement};
use std::fmt;
use std::sync::Arc;

use crate::{effect, program, task, widget};

use super::{
    catalog, choices, preview_candidate, Host, Message, Preview, Selection, State, Storage,
};

pub(super) struct Outcome {
    evidence: Box<[compatibility::Evidence]>,
    task: iced::Task<Message>,
    inputs: Option<rawr::reload::scheduler::publication::Retired>,
}

pub(super) struct Draft {
    generation: rawr::program::GenerationId,
    control_previews: Box<[crate::control::Preview]>,
    source: rawr::diagnostic::Source,
    choices: Vec<super::Choice>,
    selection: Selection,
    application: Option<program::Draft>,
    preview: Option<Preview>,
    last_good_theme: crate::theme::Value,
    failure: Option<rawr::diagnostic::Report>,
    task: Option<task::Prepared>,
}

#[derive(Debug)]
pub(crate) enum Error {
    Unavailable,
    Binding(installation::Error),
    Owner,
    Missing,
    Candidate(rawr::diagnostic::Report),
    Application(program::install::Error),
    Task(task::Error),
    Diagnostic(rawr::diagnostic::Report),
    Source(rawr::reload::source::Error),
    Stale,
}

impl Host {
    pub(super) fn install(
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
            let generation = match self.generation() {
                Some(generation) => generation,
                None => return Err(candidate.map(|_| Error::Unavailable)),
            };
            let owners = match owners(self) {
                Ok(owners) => owners,
                Err(error) => return Err(candidate.map(|_| error)),
            };
            let candidate = candidate.map(|(candidate, inputs)| {
                (
                    compatibility::Plan::compare(generation, candidate, owners),
                    inputs,
                )
            });
            let candidate = candidate.map_result(|(plan, inputs)| {
                plan.bind(engine, runtime)
                    .map(|bound| (bound, inputs))
                    .map_err(Error::Binding)
            })?;
            let candidate = candidate.map_result(|(replacement, inputs)| {
                self.prepare(replacement).map(|prepared| (prepared, inputs))
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
        let Storage::Reload(current) = &mut self.storage else {
            return Err(Error::Unavailable);
        };
        let admit = || gate.admit_slot_and_publish(candidate, current, publication);
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
                let task = self.activate_application();
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
                    inputs: Some(inputs),
                }))
            }
            rawr::reload::scheduler::Admission::Current(Err(rejected)) => {
                drop(rejected.release());
                Err(Error::Stale)
            }
            rawr::reload::scheduler::Admission::Obsolete(prepared) => {
                drop(prepared);
                Ok(rawr::reload::scheduler::Admission::Obsolete(()))
            }
        }
    }

    #[cfg(test)]
    pub(super) fn prepare_for_test(
        &self,
        replacement: Replacement,
    ) -> Result<installation::current::Prepared<State>, Error> {
        self.prepare(replacement)
    }

    #[cfg(test)]
    pub(super) fn commit_for_test(
        &mut self,
        prepared: installation::current::Prepared<State>,
    ) -> Result<Outcome, Error> {
        let now = std::time::Instant::now();
        let gate = rawr::reload::scheduler::Gate::new(std::time::Duration::ZERO);
        gate.observe(now).map_err(|_| Error::Stale)?;
        let candidate = gate.start(now).ok_or(Error::Stale)?.complete(prepared);
        let Storage::Reload(current) = &mut self.storage else {
            return Err(Error::Unavailable);
        };
        let (evidence, retired) = match gate.admit_slot(candidate, current) {
            rawr::reload::scheduler::Admission::Current(Ok(admitted)) => admitted,
            rawr::reload::scheduler::Admission::Current(Err(rejected)) => {
                drop(rejected.release());
                return Err(Error::Stale);
            }
            rawr::reload::scheduler::Admission::Obsolete(prepared) => {
                drop(prepared);
                return Err(Error::Stale);
            }
        };
        let task = self.activate_application();
        let old = retired.release();
        drop(old);
        Ok(Outcome {
            evidence,
            task,
            inputs: None,
        })
    }

    #[cfg(test)]
    pub(super) fn install_replacement_for_test(
        &mut self,
        replacement: Replacement,
    ) -> Result<Outcome, Error> {
        let prepared = self.prepare(replacement)?;
        self.commit_for_test(prepared)
    }

    fn prepare(
        &self,
        replacement: Replacement,
    ) -> Result<installation::current::Prepared<State>, Error> {
        let owners = owners(self)?;
        if replacement.evidence().len() != owners.iter().len()
            || owners
                .iter()
                .any(|owner| replacement.owner(owner).is_none())
        {
            return Err(Error::Owner);
        }
        let Storage::Reload(current) = &self.storage else {
            return Err(Error::Unavailable);
        };
        let source = replacement
            .generation()
            .source()
            .entry_source()
            .diagnostic()
            .clone();
        let application_action = replacement
            .owner(&Owner::Application)
            .map(compatibility::Evidence::action);
        let preview_action = self.preview.as_ref().and_then(|preview| {
            replacement
                .owner(&Owner::Preview(preview.key.clone()))
                .map(compatibility::Evidence::action)
        });
        let preparing = replacement.prepare(current).map_err(|_| Error::Stale)?;
        let state = {
            let preparation = preparing.preparation();
            Draft::replacement(
                preparing.state(),
                preparation.catalog(),
                source,
                application_action,
                preview_action,
                self.timing.as_ref(),
            )?
            .seal()
        };
        Ok(preparing.seal(state))
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

impl Draft {
    pub(super) fn bootstrap(
        catalog: installation::Catalog<'_>,
        source: rawr::diagnostic::Source,
        timing: Option<&rawr::timing::Session>,
    ) -> Result<Self, rawr::diagnostic::Report> {
        let choices = choices(&catalog);
        let control_previews = catalog
            .previews()
            .iter()
            .map(crate::control::Preview::from_definition)
            .collect();
        let selection = choices
            .first()
            .map(|choice| choice.selection.clone())
            .ok_or_else(|| {
                super::plain_report("preview Catalog contains no selectable entries", &source)
            })?;
        let mut application = None;
        let mut preview = None;
        let mut last_good_theme = crate::theme::Value::default();
        let mut failure = None;
        let mut task = None;
        match &selection {
            Selection::Application => match catalog.application() {
                Some(launch) => {
                    match program::Draft::boot(&launch, Some(source.clone()), timing.cloned()) {
                        Ok(mut draft) => {
                            draft.clear_timing();
                            last_good_theme = draft.theme().clone();
                            let startup = draft.take_startup_plan();
                            match startup.prepare_application(&launch) {
                                Ok(prepared) => {
                                    task = Some(prepared);
                                    application = Some(draft);
                                }
                                Err(error) => {
                                    failure = Some(super::plain_report(&error.to_string(), &source))
                                }
                            }
                        }
                        Err(error) => failure = Some(error.report(Some(source.clone()))),
                    }
                }
                None => {
                    failure = Some(super::plain_report(
                        "Catalog has no application selection",
                        &source,
                    ))
                }
            },
            Selection::Preview(key) => {
                match preview_candidate(&catalog, key, None, &last_good_theme, &source, timing) {
                    Ok(session) => {
                        last_good_theme = session.theme.clone();
                        preview = Some(session);
                    }
                    Err(report) => failure = Some(report),
                }
            }
        }
        Ok(Self {
            generation: catalog.generation(),
            control_previews,
            source,
            choices,
            selection,
            application,
            preview,
            last_good_theme,
            failure,
            task,
        })
    }

    fn replacement(
        previous: &State,
        catalog: installation::Catalog<'_>,
        source: rawr::diagnostic::Source,
        application_action: Option<Action>,
        preview_action: Option<Action>,
        timing: Option<&rawr::timing::Session>,
    ) -> Result<Self, Error> {
        let mut task = None;
        let application = match &previous.application {
            Some(adapter) => {
                let action = application_action.ok_or(Error::Owner)?;
                let launch = catalog.application().ok_or(Error::Missing)?;
                let mut draft = match action {
                    Action::Retain => {
                        program::install::retain(adapter, &launch, Some(source.clone()), timing)
                            .map_err(Error::Application)?
                    }
                    Action::Reset => {
                        let mut draft =
                            program::Draft::boot(&launch, Some(source.clone()), timing.cloned())
                                .map_err(program::install::Error::Adapter)
                                .map_err(Error::Application)?;
                        if let Some(entry) = draft.diagnostics().first() {
                            return Err(Error::Candidate(entry.report().clone()));
                        }
                        draft.clear_timing();
                        draft
                    }
                };
                let startup = draft.take_startup_plan();
                task = Some(startup.prepare_application(&launch).map_err(Error::Task)?);
                Some(draft)
            }
            None => None,
        };
        let inherited = application
            .as_ref()
            .map_or_else(crate::theme::Value::default, |application| {
                application.theme().clone()
            });
        let preview = match &previous.preview {
            Some(old) => {
                let action = preview_action.ok_or(Error::Owner)?;
                if !catalog
                    .previews()
                    .iter()
                    .any(|definition| definition.key() == &old.key)
                {
                    return Err(Error::Missing);
                }
                Some(match action {
                    Action::Retain => retain_preview(old, &catalog, &inherited, timing)?,
                    Action::Reset => preview_candidate(
                        &catalog,
                        &old.key,
                        old.selected_theme,
                        &inherited,
                        &source,
                        timing,
                    )
                    .map_err(Error::Candidate)?,
                })
            }
            None => None,
        };
        let last_good_theme = match &previous.selection {
            Selection::Application => application
                .as_ref()
                .map(|application| application.theme().clone())
                .ok_or(Error::Missing)?,
            Selection::Preview(_) => preview
                .as_ref()
                .map(|preview| preview.theme.clone())
                .ok_or(Error::Missing)?,
        };
        Ok(Self {
            generation: catalog.generation(),
            choices: choices(&catalog),
            control_previews: catalog
                .previews()
                .iter()
                .map(crate::control::Preview::from_definition)
                .collect(),
            source,
            selection: previous.selection.clone(),
            application,
            preview,
            last_good_theme,
            failure: None,
            task,
        })
    }

    pub(super) fn seal(self) -> State {
        let Self {
            generation,
            control_previews,
            source,
            choices,
            selection,
            application,
            preview,
            last_good_theme,
            failure,
            task,
        } = self;
        let mut application = application.map(|draft| draft.install(generation));
        if let (Some(task), Some(application)) = (task, application.as_mut()) {
            application.prepared = Some(task)
        }
        State {
            control_previews,
            source,
            choices,
            selection,
            application,
            preview,
            last_good_theme,
            failure,
            pending: None,
            ready: None,
            pending_settles: Vec::new(),
        }
    }
}

impl Outcome {
    pub(super) const fn evidence(&self) -> &[compatibility::Evidence] {
        &self.evidence
    }

    #[cfg(test)]
    pub(super) fn into_task(self) -> iced::Task<Message> {
        let (task, inputs) = self.into_parts();
        if let Some(inputs) = inputs {
            inputs.release();
        }
        task
    }

    pub(super) fn into_parts(
        self,
    ) -> (
        iced::Task<Message>,
        Option<rawr::reload::scheduler::publication::Retired>,
    ) {
        (self.task, self.inputs)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("preview host has no reload generation proof"),
            Self::Binding(error) => error.fmt(formatter),
            Self::Owner => {
                formatter.write_str("replacement does not prove the selected preview owner")
            }
            Self::Missing => {
                formatter.write_str("replacement is missing one retained preview host owner")
            }
            Self::Candidate(report) => report.fmt(formatter),
            Self::Application(error) => error.fmt(formatter),
            Self::Task(error) => error.fmt(formatter),
            Self::Diagnostic(report) => report.fmt(formatter),
            Self::Source(error) => error.fmt(formatter),
            Self::Stale => {
                formatter.write_str("replacement no longer matches the exact preview host state")
            }
        }
    }
}

impl Error {
    pub(super) fn report(
        &self,
        source: Option<rawr::diagnostic::Source>,
    ) -> rawr::diagnostic::Report {
        match self {
            Self::Candidate(report) | Self::Diagnostic(report) => report.clone(),
            Self::Application(error) => error.report(source),
            Self::Binding(_) => crate::reload::report(
                rawr::diagnostic::code::FRAMEWORK_RUNTIME_BINDING,
                self,
                source,
            ),
            Self::Source(_) => {
                crate::reload::report(rawr::diagnostic::code::ENVIRONMENT_DIAGNOSTIC, self, source)
            }
            Self::Unavailable | Self::Owner | Self::Missing | Self::Task(_) | Self::Stale => {
                crate::reload::report(rawr::diagnostic::code::FRAMEWORK_NATIVE, self, source)
            }
        }
    }
}

pub(super) fn owners(host: &Host) -> Result<Owners, Error> {
    let mut owners = Vec::new();
    if host.application.is_some() {
        owners.push(Owner::Application);
    }
    if let Some(preview) = &host.preview {
        owners.push(Owner::Preview(preview.key.clone()));
    }
    Owners::new(owners).map_err(|_| Error::Unavailable)
}

fn retain_preview(
    preview: &Preview,
    catalog: &dyn catalog::Evaluation,
    inherited: &crate::theme::Value,
    timing: Option<&rawr::timing::Session>,
) -> Result<Preview, Error> {
    let state = preview.state.clone();
    let theme = match preview.selected_theme {
        Some(index) => catalog
            .theme(&preview.key, index, state.as_ref())
            .map_err(|error| {
                super::preview_report(catalog, &preview.key, super::Stage::Theme(index), error)
            })
            .and_then(|value| {
                crate::theme::Value::from_dynamic(value).map_err(|error| {
                    super::positioned_report(
                        catalog,
                        &preview.key,
                        super::Stage::Theme(index),
                        rawr::diagnostic::code::FRAMEWORK_THEME_OUTPUT,
                        error.to_string(),
                    )
                })
            })
            .map_err(Error::Candidate)?,
        None => inherited.clone(),
    };
    let evaluate = || {
        catalog
            .subject(&preview.key, state.as_ref())
            .map_err(|error| {
                super::preview_report(catalog, &preview.key, super::Stage::Subject, error)
            })
            .map_err(Error::Candidate)
            .and_then(|value| {
                widget::Value::from_dynamic(value).map_err(|error| {
                    Error::Candidate(super::positioned_report(
                        catalog,
                        &preview.key,
                        super::Stage::Subject,
                        rawr::diagnostic::code::FRAMEWORK_VIEW_OUTPUT,
                        error.to_string(),
                    ))
                })
            })
    };
    let display = match timing {
        Some(timing) => timing.observe_result(rawr::timing::Phase::ViewEvaluation, evaluate),
        None => evaluate(),
    }?;
    let lifecycle = Arc::new(widget::diagnostic::Sink::new(
        preview.instance,
        catalog.generation(),
    ));
    Ok(Preview {
        key: preview.key.clone(),
        instance: preview.instance,
        lifecycle,
        state,
        display,
        theme,
        selected_theme: preview.selected_theme,
        diagnostics: Vec::new(),
        tasks: effect::Tasks::default(),
    })
}
