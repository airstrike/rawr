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
    pub(super) fn set_variable(&self, arguments: &Value) -> Result<Value, String> {
        let name = arguments["name"]
            .as_str()
            .ok_or("setVariable needs the variable's name")?;
        let (frame, place) = match self.handle(arguments)? {
            Handle::Frame(frame) => (frame, Place::binding(name)),
            Handle::Parts(parts) => (parts.frame, parts.place_of(name)),
        };
        self.assign(arguments, frame, place)
    }

    /// `setExpression`: the place `expression` names in frame `frameId`
    /// assigned `value`.
    pub(super) fn set_expression(&self, arguments: &Value) -> Result<Value, String> {
        let expression = arguments["expression"]
            .as_str()
            .ok_or("setExpression needs an expression")?;
        let frame = arguments["frameId"].as_u64().unwrap_or(1).saturating_sub(1);
        self.assign(arguments, frame, Place::expression(expression))
    }

    /// `arguments`' `value` assigned to `place` in frame `frame`: answered
    /// with the value the place then holds, and its parts behind a
    /// reference.
    fn assign(&self, arguments: &Value, frame: u64, place: Place) -> Result<Value, String> {
        let value = arguments["value"].as_str().ok_or("a new value is needed")?;
        let variable = self.eval(frame, &place.assign(value), place)?;
        Ok(json!({
            "value": variable["value"],
            "variablesReference": variable["variablesReference"],
        }))
    }
}
