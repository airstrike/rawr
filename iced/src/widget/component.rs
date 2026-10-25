//! iced's `component`: a script type implementing `component::Component`,
//! backed by iced's own component widget, so its State lives in the widget
//! tree and the Events of its view reach its `update`, whose Message, if
//! any, the widget publishes.

use iced::widget::Widget as _;
use rawr::{host, Dynamic, EvalAltResult};

use super::builtin::{arity, take, witness_error};
use crate::{marker, runtime};

pub(super) fn bind(builder: &mut host::runtime::Builder) {
    builder
        .bind_with_context("iced::widget::component::Instance::new", |call, args| {
            if args.len() != 4 {
                return Err(arity("component::Instance::new", 4, args.len()));
            }
            Ok(Dynamic::from(marker::Component {
                witness: call.output_witness().map_err(witness_error)?,
                identity: super::Identity::unique(),
                value: args[0].take(),
                update: take(args[1], "component update")?,
                view: take(args[2], "component view")?,
                state: args[3].take(),
            }))
        })
        .expect("the iced facade must retain the exact component Instance item");
    builder
        .bind_with_context("iced::widget::component", |call, args| {
            if args.len() != 1 {
                return Err(arity("component", 1, args.len()));
            }
            let component = take::<marker::Component>(args[0], "component::Instance")?;
            super::Value::from_factory(component, call)
                .map(Dynamic::from)
                .map_err(witness_error)
        })
        .expect("the iced facade must retain the exact component item");
    builder
        .bind_type_attestation("iced::widget::component::Instance", |value, expected| {
            let value = value.read_lock::<marker::Component>()?;
            (expected.argument_count() == 1)
                .then(|| expected.attest_witness(Some(&value.witness)))
                .flatten()
        })
        .expect("the iced facade must declare generic component::Instance exactly once");
}

impl super::Descriptor for marker::Component {
    fn lower<'a>(&self, context: &super::Context<'a>) -> iced::Element<'a, runtime::Message> {
        iced::widget::component(Instance {
            component: self.clone(),
            context: context.clone(),
        })
        .boxed()
    }

    fn identity(&self) -> super::Identity {
        self.identity.clone()
    }
}

struct Instance<'a> {
    component: marker::Component,
    context: super::Context<'a>,
}

/// The tree-held State: the script State once the component has one, and
/// the failure of its last `update`, which its view shows.
#[derive(Default)]
pub(crate) struct State {
    value: Option<Dynamic>,
    failure: Option<String>,
}

impl<'a> Instance<'a> {
    fn context(&self) -> super::Context<'a> {
        self.context.clone()
    }

    fn state(&self, state: &State) -> Dynamic {
        state
            .value
            .clone()
            .unwrap_or_else(|| self.component.state.clone())
    }
}

impl<'a> iced::widget::component::Component<'a, runtime::Message> for Instance<'a> {
    type State = State;
    type Event = runtime::Message;

    fn update(
        &self,
        state: &mut State,
        event: runtime::Message,
        _renderer: &iced::Renderer,
    ) -> Option<runtime::Message> {
        let context = self.context();
        let mut arguments = [
            self.component.value.clone(),
            self.state(state),
            event.into_payload(),
            Dynamic::from(marker::Renderer),
        ];
        // `update` copies its `&mut State` back out: `[message, state]`.
        let changed = context
            .invoke(&self.component.update, &mut arguments)?
            .and_then(|changed| {
                let changed = changed
                    .try_cast_result::<rawr::Array>()
                    .map_err(|value| format!("returned `{}`", value.type_name()))?;
                let [mut message, value] = <[Dynamic; 2]>::try_from(changed)
                    .map_err(|changed| format!("returned {} values", changed.len()))?;
                let message = rawr::option::take(&mut message)
                    .ok_or_else(|| format!("returned `{}`", message.type_name()))?;
                Ok((message, value))
            });
        match changed {
            Ok((message, value)) => {
                state.value = Some(value);
                state.failure = None;
                message.map(|message| context.message(message))
            }
            Err(error) => {
                state.failure = Some(format!("component update failed: {error}"));
                None
            }
        }
    }

    fn view(&self, state: &State) -> impl iced::widget::Widget<runtime::Message> + 'a {
        let context = self.context();
        if let Some(failure) = &state.failure {
            return super::diagnostic(failure.clone()).lower(&context);
        }
        let mut arguments = [self.component.value.clone(), self.state(state)];
        match context.invoke(&self.component.view, &mut arguments) {
            None => iced::widget::void().boxed(),
            Some(Ok(mut view)) => match take::<super::Value>(&mut view, "iced::Element") {
                Ok(view) => view.lower(&context.child_mapping()),
                Err(error) => component_failure(&context, &error),
            },
            Some(Err(error)) => {
                super::diagnostic(format!("component view failed: {error}")).lower(&context)
            }
        }
    }
}

fn component_failure<'a>(
    context: &super::Context<'a>,
    error: &EvalAltResult,
) -> iced::Element<'a, runtime::Message> {
    super::diagnostic(format!("component view failed: {error}")).lower(context)
}
