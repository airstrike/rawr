//! What the client changes where the program is paused: a variable it
//! read, or an expression it watches, given a new value. Each is the
//! inspector's `eval` of `place = value` in the value's frame, so the
//! program's compiler allows the write where Rust does and refuses it with
//! rustc's error where Rust does (D1076.1), and the program goes on with
//! the new value.

use serde_json::{json, Value};

use super::handles::Handle;
use super::place::Place;
use super::Adapter;

impl Adapter {
    /// `setVariable`: the variable `name` behind `variablesReference`, a
    /// frame's binding or a value's part, assigned `value`.
    pub(super) fn set_variable(&mut self, request: Value) {
        let arguments = &request["arguments"];
        let Some(name) = arguments["name"].as_str() else {
            let refusal = Err("setVariable needs the variable's name".to_owned());
            return self.out.respond(&request, refusal);
        };
        let (frame, place) = match self.handle(arguments) {
            Ok(Handle::Frame(frame)) => (frame, Place::binding(name)),
            Ok(Handle::Parts(parts)) => (parts.frame, parts.place_of(name)),
            Err(message) => return self.out.respond(&request, Err(message)),
        };
        self.assign(request, frame, place);
    }

    /// `setExpression`: the place `expression` names in frame `frameId`
    /// assigned `value`.
    pub(super) fn set_expression(&mut self, request: Value) {
        let arguments = &request["arguments"];
        let Some(expression) = arguments["expression"].as_str() else {
            let refusal = Err("setExpression needs an expression".to_owned());
            return self.out.respond(&request, refusal);
        };
        let frame = arguments["frameId"].as_u64().unwrap_or(1).saturating_sub(1);
        let place = Place::expression(expression);
        self.assign(request, frame, place);
    }

    /// The request's `value` assigned to `place` in frame `frame`: answered
    /// with the value the place then holds, and its parts behind a
    /// reference.
    fn assign(&mut self, request: Value, frame: u64, place: Place) {
        let Some(value) = request["arguments"]["value"].as_str() else {
            let refusal = Err("a new value is needed".to_owned());
            return self.out.respond(&request, refusal);
        };
        let code = place.assign(value);
        self.eval(frame, &code, place, move |adapter, variable| {
            let answer = variable.map(|variable| {
                json!({
                    "value": variable["value"],
                    "variablesReference": variable["variablesReference"],
                })
            });
            adapter.out.respond(&request, answer);
        });
    }
}
