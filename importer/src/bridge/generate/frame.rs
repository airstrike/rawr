//! The frame of a script impl function generic over type parameters of a
//! crate trait's method, which native code fills, as `S` of a
//! `serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok,
//! S::Error>` (#593).
//!
//! The runner's impl function is generic as the trait's method is. Each
//! value of the opaque type of a parameter or of one of its associated
//! types, which the script function takes, makes or gives back, stays in
//! the call's frame, where rustc knows its type: it needn't be `'static`,
//! as a value in the arena must. The frame is lent to the script function
//! for the call, and script code holds each value by a handle to it
//! (`FrameRef`). A call on one, as `serializer.serialize_struct(..)`, runs
//! the frame's entry for it, compiled in the frame at its types; its
//! adapter reaches the entry through the handle's table.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::{convert, request, Plan};

/// A generic script impl function's frame, as the runner declares it.
pub(super) struct Frame {
    /// The frame type's name.
    name: String,
    /// The slot of each opaque type, by its identity.
    slots: BTreeMap<[u8; 32], usize>,
    /// Each slot's opaque type and its Rust type in the frame.
    types: Vec<(convert::Type, String)>,
    /// The frame's type parameters with their bounds, as the impl's own
    /// then the function's.
    header: String,
    /// The frame's type parameters, by name.
    arguments: String,
    /// The function's own type parameters with their bounds.
    own: String,
}

impl Frame {
    /// The frame of `function` of `script`, with a slot for each opaque type
    /// of `function`'s own parameters that its signature or a call in the
    /// cohort names.
    pub(super) fn of(
        script: &crate::surface::instance::ScriptImpl,
        function: &crate::surface::instance::ScriptImplFunction,
        cohort: &super::Cohort<'_>,
    ) -> Result<Self, request::Error> {
        let drift = request::drift;
        let trait_path = root_spelling(script.trait_path());
        let mut found = BTreeMap::<(usize, Option<String>), convert::Type>::new();
        let mut named = Vec::new();
        for plan in cohort.plans {
            named.extend(plan_types(plan)?);
        }
        named.extend(function.inputs().iter().cloned());
        named.push(function.output().clone());
        let mut nominals = Vec::new();
        for ty in &named {
            nominal_types(ty, &mut nominals);
        }
        for ty in nominals {
            let convert::Type::Nominal { definition, .. } = &ty else {
                continue;
            };
            let Some(path) =
                super::TYPE_PATHS.with_borrow(|paths| paths.get(definition.as_bytes()).cloned())
            else {
                continue;
            };
            if path
                .rsplit_once("::__rawr_self::")
                .is_none_or(|(owner, _)| root_spelling(owner) != trait_path)
            {
                continue;
            }
            let Some((method, index, projection)) = crate::surface::opaque::parameter_of(&path)
            else {
                continue;
            };
            if method != function.name() {
                continue;
            }
            found
                .entry((index, projection.map(str::to_owned)))
                .or_insert(ty.clone());
        }
        let generics = function.generics();
        let mut slots = BTreeMap::new();
        let mut types = Vec::new();
        for ((index, projection), ty) in found {
            let generic = generics
                .get(index)
                .ok_or_else(|| drift("a generic impl function's opaque parameter"))?;
            let rust = match projection {
                Some(associated) => format!("{}::{associated}", generic.name()),
                None => generic.name().to_owned(),
            };
            let convert::Type::Nominal { definition, .. } = &ty else {
                continue;
            };
            slots.insert(*definition.as_bytes(), types.len());
            types.push((ty, rust));
        }
        let mut header = named_lifetimes(script)
            .unwrap_or_default()
            .iter()
            .chain(script.generics())
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let mut arguments = header.clone();
        let mut own = Vec::new();
        for generic in generics {
            let mut bounds = Vec::new();
            for bound in generic.bounds() {
                // A lifetime the bound names is the impl's, by its name.
                if !bound.lifetimes().is_empty() && named_lifetimes(script).is_none() {
                    return Err(drift(
                        "a generic impl function's bound that takes a lifetime",
                    ));
                }
                let written = bound
                    .lifetimes()
                    .iter()
                    .map(ToString::to_string)
                    .map(Some)
                    .chain(
                        bound
                            .arguments()
                            .iter()
                            .map(|argument| super::impl_type(argument, script.generics())),
                    )
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| drift("a generic impl function's bound's argument"))?;
                bounds.push(if written.is_empty() {
                    bound.path().to_owned()
                } else {
                    format!("{}<{}>", bound.path(), written.join(", "))
                });
            }
            let written = if bounds.is_empty() {
                generic.name().to_owned()
            } else {
                format!("{}: {}", generic.name(), bounds.join(" + "))
            };
            header.push(written.clone());
            own.push(written);
            arguments.push(generic.name().to_owned());
        }
        let name = format!(
            "{PREFIX}{}",
            function
                .member()
                .chars()
                .map(|character| if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '_'
                })
                .collect::<String>()
        );
        Ok(Self {
            name,
            slots,
            types,
            header: header.join(", "),
            arguments: arguments.join(", "),
            own: own.join(", "),
        })
    }

    /// The function's own type parameters with their bounds, as its header
    /// writes them.
    pub(super) fn function_header(&self) -> String {
        format!("<{}>", self.own)
    }

    /// The slot of the opaque type `ty`, when the frame keeps it.
    pub(super) fn slot(&self, ty: &convert::Type) -> Option<usize> {
        let convert::Type::Nominal { definition, .. } = ty else {
            return None;
        };
        self.slots.get(definition.as_bytes()).copied()
    }

    /// The slots by identity, as `InFrame` takes them.
    pub(super) fn slots(&self) -> &BTreeMap<[u8; 32], usize> {
        &self.slots
    }

    /// The static of each opaque type's slot, by identity, as
    /// `FrameCurrent` takes it.
    pub(super) fn keys(&self) -> String {
        format!("{}{KEYS}", self.name)
    }

    /// An empty frame of the call, at its type parameters.
    pub(super) fn made(&self) -> String {
        format!("{}::<{}>::new()", self.name, self.arguments)
    }

    /// `ty` as Rust spells it in the function's signature, with each
    /// opaque type the frame keeps its slot's type.
    pub(super) fn spell(&self, ty: &convert::Type, generics: &[Box<str>]) -> Option<String> {
        if let Some(slot) = self.slot(ty) {
            return Some(self.types[slot].1.clone());
        }
        let all = |types: &[convert::Type]| {
            types
                .iter()
                .map(|ty| self.spell(ty, generics))
                .collect::<Option<Vec<_>>>()
        };
        if !self.names_slot(ty) {
            return super::impl_type(ty, generics);
        }
        Some(match ty {
            convert::Type::Result(ok, err) => format!(
                "Result<{}, {}>",
                self.spell(ok, generics)?,
                self.spell(err, generics)?
            ),
            convert::Type::Option(inner) => format!("Option<{}>", self.spell(inner, generics)?),
            convert::Type::Borrowed { mutable, ty } => format!(
                "&{}{}",
                if *mutable { "mut " } else { "" },
                self.spell(ty, generics)?
            ),
            convert::Type::Tuple(members) => format!("({},)", all(members)?.join(", ")),
            convert::Type::Sequence { element, .. } => {
                format!("Vec<{}>", self.spell(element, generics)?)
            }
            _ => return None,
        })
    }

    /// Whether `ty` names an opaque type the frame keeps, at any depth.
    fn names_slot(&self, ty: &convert::Type) -> bool {
        let mut found = Vec::new();
        nominal_types(ty, &mut found);
        found.iter().any(|ty| self.slot(ty).is_some())
    }

    /// The frame's declaration, its accessors, its calls and the tables
    /// that reach them.
    pub(super) fn render(
        &self,
        script: &crate::surface::instance::ScriptImpl,
        cohort: &super::Cohort<'_>,
    ) -> Result<String, request::Error> {
        let Self {
            name,
            header,
            arguments,
            ..
        } = self;
        let impl_parameters = named_lifetimes(script)
            .unwrap_or_default()
            .iter()
            .map(|lifetime| format!("&{lifetime} ()"))
            .chain(script.generics().iter().map(ToString::to_string))
            .collect::<Vec<_>>()
            .join(", ");
        let mut fields = String::new();
        let mut made = String::new();
        let mut accessors = String::new();
        let mut entries = String::new();
        let mut tables = String::new();
        for (slot, (ty, rust)) in self.types.iter().enumerate() {
            let _ = writeln!(fields, "    s{slot}: Vec<Option<{rust}>>,");
            let _ = write!(made, " s{slot}: Vec::new(),");
            let _ = write!(
                accessors,
                "
    fn keep_{slot}(&mut self, value: {rust}) -> usize {{
        self.s{slot}.push(Some(value));
        self.s{slot}.len() - 1
    }}

    fn out_{slot}(&mut self, arena: &mut Arena, handle: NativeHandle) -> Result<{rust}, Error> {{
        let at = frame_index(arena, handle, self.nonce, {slot})?;
        self.s{slot}.get_mut(at).and_then(Option::take).ok_or(Error::StaleHandle)
    }}

    fn take_{slot}(&mut self, arguments: &mut Arguments<'_, '_, '_>, position: usize) -> Result<{rust}, Error> {{
        let handle = native_handle(arguments, position, false)?;
        self.out_{slot}(arguments.arena, handle)
    }}

    // The arena is lent while the method runs, as for an adapter's call:
    // a method that calls back into script code, as a `next_key::<K>()`
    // that reads a script type's `K`, reaches it there.
    fn shared_{slot}<RawrOutput>(&self, arena: &mut Arena, handle: NativeHandle, call: impl FnOnce(&{rust}) -> RawrOutput) -> Result<RawrOutput, Error> {{
        let at = frame_index(arena, handle, self.nonce, {slot})?;
        let value = self.s{slot}.get(at).and_then(Option::as_ref).ok_or(Error::StaleHandle)?;
        Ok(lend(arena, || call(value)))
    }}

    fn mutable_{slot}<RawrOutput>(&mut self, arena: &mut Arena, handle: NativeHandle, call: impl FnOnce(&mut {rust}) -> RawrOutput) -> Result<RawrOutput, Error> {{
        let at = frame_index(arena, handle, self.nonce, {slot})?;
        let value = self.s{slot}.get_mut(at).and_then(Option::as_mut).ok_or(Error::StaleHandle)?;
        Ok(lend(arena, || call(value)))
    }}

    fn give_{slot}(&mut self, arena: &mut Arena, value: {rust}) -> Result<Dynamic, Error> {{
        let at = self.keep_{slot}(value);
        let handle = frame_handle(arena, self.nonce, {slot}, at, {name}{TABLE}{slot}())?;
        // Before the frame is lent, its nonce is the lend's to come.
        if self.nonce == 0 {{
            self.fresh.push(handle.clone_cast::<NativeHandle>());
        }}
        Ok(handle)
    }}
"
            );
            let consumers = super::opaque_consumers(cohort.plans, ty)?;
            // A call that takes no value of a frame's but names its type,
            // as `<S::Error as Error>::custom(..)`, runs here too, after
            // those that take one (`static_entry`).
            for (offset, plan) in statics(cohort.plans, ty)?.into_iter().enumerate() {
                let entry = consumers.len() + offset;
                let body = self.entry(plan, ty, slot, usize::MAX, cohort)?;
                let _ = write!(
                    entries,
                    "            ({slot}, {entry}) => {{\n{body}\n            }}\n"
                );
            }
            let mut reach = String::new();
            for (entry, consumer) in consumers.iter().enumerate() {
                let Some((position, _, _)) = super::opaque_operand(consumer)? else {
                    continue;
                };
                let body = self.entry(consumer, ty, slot, position, cohort)?;
                let _ = write!(
                    entries,
                    "            ({slot}, {entry}) => {{\n{body}\n            }}\n"
                );
                let _ = writeln!(
                    reach,
                    "            Box::new(|arguments: &mut Arguments<'_, '_, '_>, _: OpaqueTable| frame_dispatch(arguments, {position}, {slot}, {entry})) as OpaqueEntry,"
                );
            }
            let _ = write!(
                tables,
                "
#[allow(non_snake_case)]
fn {name}{TABLE}{slot}() -> OpaqueTable {{
    static TABLE: std::sync::OnceLock<Box<[OpaqueEntry]>> = std::sync::OnceLock::new();
    OpaqueTable(TABLE.get_or_init(|| {{
        vec![
{reach}            Box::new(|_: &mut Arguments<'_, '_, '_>, _: OpaqueTable| Err(Error::TypeMismatch)) as OpaqueEntry,
        ]
        .into_boxed_slice()
    }}))
}}
"
            );
        }
        let keys = self
            .slots
            .iter()
            .map(|(identity, slot)| format!("({identity:?}, {slot})"))
            .collect::<Vec<_>>()
            .join(", ");
        Ok(format!(
            "
#[allow(non_upper_case_globals)]
static {name}{KEYS}: &[([u8; 32], u32)] = &[{keys}];

/// The frame of a generic function of the script's impl (#593).
#[allow(non_camel_case_types, dead_code)]
struct {name}<{header}> {{
    nonce: u64,
    fresh: Vec<NativeHandle>,
{fields}    parameters: std::marker::PhantomData<fn() -> ({impl_parameters})>,
}}

#[allow(dead_code, unused_mut, clippy::all)]
impl<{header}> {name}<{arguments}> {{
    fn new() -> Self {{
        Self {{ nonce: 0, fresh: Vec::new(),{made} parameters: std::marker::PhantomData }}
    }}
{accessors}}}

#[allow(unused_mut, unused_variables, unreachable_code, clippy::all)]
impl<{header}> Frame for {name}<{arguments}> {{
    fn call(&mut self, nonce: u64, slot: u32, entry: usize, arguments: &mut Arguments<'_, '_, '_>) -> Result<Dynamic, Error> {{
        self.nonce = nonce;
        let _ = &mut *arguments;
        match (slot, entry) {{
{entries}            _ => Err(Error::TypeMismatch),
        }}
    }}
}}
{tables}"
        ))
    }

    /// The body of the call `consumer` makes on the value at argument
    /// `position`, of the opaque type `ty` in `slot`: the adapter's body,
    /// with the value taken from and reached in the frame.
    fn entry(
        &self,
        consumer: &Plan,
        ty: &convert::Type,
        slot: usize,
        position: usize,
        cohort: &super::Cohort<'_>,
    ) -> Result<String, request::Error> {
        let mut body = Vec::new();
        {
            let _frame = super::InFrame::enter(&self.slots, "self");
            let _producing = super::OpaqueProducing::enter(None);
            let _consuming = super::OpaqueConsuming::enter(false);
            let _witnessed = super::OpaqueWitnessed::enter(Some(ty.clone()));
            super::render_adapter_body(
                consumer,
                cohort.plans,
                cohort.profile,
                cohort.facade,
                &mut body,
            )?;
        }
        let body = String::from_utf8(body).map_err(request::unwritable)?;
        Ok(body
            .replace(
                &format!("take_native(arguments, {position})"),
                &format!("self.take_{slot}(arguments, {position})"),
            )
            .replace(
                "shared_like(arguments.arena, receiver, _w, ",
                &format!("self.shared_{slot}(arguments.arena, receiver, "),
            )
            .replace(
                "mutable_like(arguments.arena, receiver, _w, ",
                &format!("self.mutable_{slot}(arguments.arena, receiver, "),
            ))
    }
}

/// What every frame's name starts with.
const PREFIX: &str = "RawrFrame_";
/// What a frame's static of its slots' keys adds to its name.
const KEYS: &str = "_KEYS";
/// What a frame's table of a slot adds to its name, before the slot.
const TABLE: &str = "_table_";

/// Whether a top-level item's name is a frame's or one of its parts'.
pub(super) fn is_frame(name: &str) -> bool {
    name.starts_with(PREFIX)
}

/// The frame a top-level item of a frame is part of, by its name: a
/// frame's keys and tables go where its declaration goes, so the runner's
/// crate that holds the frame holds them too (#1938). `None` for an item
/// of no frame.
pub(super) fn owner(name: &str) -> Option<&str> {
    if !is_frame(name) {
        return None;
    }
    if let Some(frame) = name.strip_suffix(KEYS) {
        return Some(frame);
    }
    let (frame, slot) = name.rsplit_once(TABLE)?;
    (!slot.is_empty() && slot.bytes().all(|byte| byte.is_ascii_digit())).then_some(frame)
}

/// The path of the trait `path` names, as its root spells it: an impl may
/// name its trait by another of its paths than the one the facade declares
/// its methods' opaque types under, as `serde::de::Deserialize` is
/// `serde::Deserialize` (#1711).
fn root_spelling(path: &str) -> String {
    super::PATH_IDENTITIES
        .with_borrow(|identities| identities.get(path).copied())
        .and_then(|identity| super::root_trait_path(&identity))
        .unwrap_or_else(|| path.to_owned())
}

/// Whether `ty` is the opaque type of a trait method's type parameter, or
/// of one of its associated types, which a frame keeps.
fn kept(ty: &convert::Type) -> bool {
    let convert::Type::Nominal { definition, .. } = ty else {
        return false;
    };
    super::TYPE_PATHS
        .with_borrow(|paths| paths.get(definition.as_bytes()).cloned())
        .is_some_and(|path| crate::surface::opaque::parameter_of(&path).is_some())
}

/// The opaque type a frame keeps that `plan` names without taking a value
/// of it, as `<S::Error as Error>::custom(..)` gives one: the first its
/// types name.
fn static_key(plan: &Plan) -> Result<Option<convert::Type>, request::Error> {
    if super::opaque_operand(plan)?.is_some() {
        return Ok(None);
    }
    let mut nominals = Vec::new();
    for ty in plan_types(plan)? {
        nominal_types(&ty, &mut nominals);
    }
    Ok(nominals.into_iter().find(kept))
}

/// The calls in `plans` that name the opaque type `key` a frame keeps but
/// take no value of it, in a fixed order.
fn statics<'p>(plans: &'p [Plan], key: &convert::Type) -> Result<Vec<&'p Plan>, request::Error> {
    let mut found = Vec::new();
    for plan in plans {
        if static_key(plan)?.is_some_and(|static_key| super::same_opaque(&static_key, key)) {
            found.push(plan);
        }
    }
    Ok(found)
}

/// The opaque type a frame keeps that `plan` names without taking a value
/// of it, and the entry of the frame's slot for it that runs the call.
pub(super) fn static_entry(
    plan: &Plan,
    cohort: &[Plan],
) -> Result<Option<(convert::Type, usize)>, request::Error> {
    let Some(key) = static_key(plan)? else {
        return Ok(None);
    };
    let consumers = super::opaque_consumers(cohort, &key)?.len();
    let index = statics(cohort, &key)?
        .iter()
        .position(|found| std::ptr::eq(*found, plan))
        .ok_or_else(|| request::drift("a frame's call"))?;
    Ok(Some((key, consumers + index)))
}

/// The names of `script`'s trait's lifetime arguments, as its header writes
/// them, where a bound of one of its generic functions names one: the
/// header names them then, so the function's own bounds can (#593).
pub(super) fn named_lifetimes(
    script: &crate::surface::instance::ScriptImpl,
) -> Option<&[Box<str>]> {
    let names = script.lifetime_names();
    let named = script.functions().iter().any(|function| {
        function.generics().iter().any(|generic| {
            generic
                .bounds()
                .iter()
                .any(|bound| !bound.lifetimes().is_empty())
        })
    });
    (named
        && names.len() == script.lifetimes() as usize
        && names.iter().all(|name| &**name != "'_"))
    .then_some(names)
}

/// The types a plan's call takes and gives, at its substitutions.
fn plan_types(plan: &Plan) -> Result<Vec<convert::Type>, request::Error> {
    use crate::surface::adapter::Operation;
    let (signature, substitutions) = match plan.operation() {
        Operation::Function {
            signature,
            substitutions,
            ..
        }
        | Operation::Method {
            signature,
            substitutions,
            ..
        } => (signature, substitutions),
        _ => return Ok(Vec::new()),
    };
    let mut types = Vec::new();
    for ty in signature.inputs().iter().chain([signature.output()]) {
        types.push(super::substituted(ty, signature, substitutions)?);
    }
    if let Some(instance) = signature.instance() {
        types.push(super::substituted(instance, signature, substitutions)?);
    }
    Ok(types)
}

/// Each nominal type `ty` names, at any depth.
fn nominal_types(ty: &convert::Type, found: &mut Vec<convert::Type>) {
    match ty {
        convert::Type::Nominal { arguments, .. } => {
            found.push(ty.clone());
            for argument in arguments.iter() {
                nominal_types(argument, found);
            }
        }
        convert::Type::Script { arguments, .. } | convert::Type::Tuple(arguments) => {
            for argument in arguments.iter() {
                nominal_types(argument, found);
            }
        }
        convert::Type::Option(inner)
        | convert::Type::Range(inner)
        | convert::Type::Borrowed { ty: inner, .. }
        | convert::Type::Sequence { element: inner, .. }
        | convert::Type::ConstArray { element: inner, .. }
        | convert::Type::ProfileMessage { payload: inner, .. } => nominal_types(inner, found),
        convert::Type::Result(ok, err) => {
            nominal_types(ok, found);
            nominal_types(err, found);
        }
        convert::Type::FunctionItem { input, output } => {
            nominal_types(input, found);
            nominal_types(output, found);
        }
        convert::Type::Object(object) => {
            for argument in object.arguments() {
                nominal_types(argument, found);
            }
        }
        convert::Type::Unit
        | convert::Type::Never
        | convert::Type::Builtin(_)
        | convert::Type::Parameter(_)
        | convert::Type::Inferred => {}
    }
}
