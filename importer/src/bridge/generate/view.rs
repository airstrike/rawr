//! Imported iced view values (D558 tier 2).
//!
//! An iced type whose lifetime is iced's view lifetime, as pane_grid's
//! `PaneGrid<'a, ..>` and `Content<'a, ..>`, has no value outside the view
//! that borrows the application's home. In an iced runner a call that makes
//! one keeps a recipe instead (`rawr_iced::widget::recipe`): the function
//! of the lowering context that makes the value at whatever view lifetime
//! the context lends. A builder call extends the recipe, and a view shows
//! the value as the element its recipe makes at each lowering.
//!
//! What the recipe replays is decided at the call:
//!
//! - an iced widget the script passes (`text(..)`, `row![..]`) is the
//!   element it lowers to;
//! - another view value is its recipe;
//! - `&T` of a native value, as `PaneGrid::new` borrows the `State`, is a
//!   copy the call keeps in the home, which the lowering context lends;
//! - a closure that makes view values, as `PaneGrid::new`'s pane closure,
//!   runs at the call (D546.2): the native constructor runs once there with
//!   a dry lowering context, and each value the closure gave is replayed in
//!   the same order at each lowering;
//! - a closure that gives a message, as `on_click(Message::Clicked)`, runs
//!   when iced calls it, and its message is stamped for the application;
//! - any other value is taken at the call and cloned into each replay.

use super::*;

/// The Rust path of rawr-iced's recipes in a generated runner.
const RECIPE: &str = "__rawr_profile_host::widget::recipe";
/// iced's message of a Rawr application, which a view value's `Message`
/// argument is.
const MESSAGE: &str = "__rawr_profile_host::runtime::Message";

/// What a generated recipe needs of an imported view type.
#[derive(Clone)]
struct ViewType {
    lifetimes: u32,
    widget: bool,
    parameters: Vec<String>,
}

thread_local! {
    /// The facade plan's view types, by identity.
    static VIEW_TYPES: std::cell::RefCell<std::collections::BTreeMap<[u8; 32], ViewType>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
    /// The recipe families the adapters name, by their type at `'a`.
    static VIEW_FAMILIES: std::cell::RefCell<std::collections::BTreeMap<String, String>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
}

/// Fill the view types of one generation's facade plan.
pub(super) fn install(facade: Option<&crate::surface::facade::Plan>) {
    VIEW_TYPES.with_borrow_mut(|types| {
        types.clear();
        for owner in facade
            .into_iter()
            .flat_map(crate::surface::facade::Plan::facades)
        {
            for ty in owner.types().filter(|ty| ty.view()) {
                types.entry(*ty.identity()).or_insert_with(|| ViewType {
                    lifetimes: ty.lifetimes(),
                    widget: ty.widget(),
                    parameters: ty.parameters().map(str::to_owned).collect(),
                });
            }
        }
    });
    VIEW_FAMILIES.with_borrow_mut(std::collections::BTreeMap::clear);
}

/// Forget the view types and families of the generation just generated.
pub(super) fn clear() {
    VIEW_TYPES.with_borrow_mut(std::collections::BTreeMap::clear);
    VIEW_FAMILIES.with_borrow_mut(std::collections::BTreeMap::clear);
}

fn view_type(definition: &crate::surface::identity::Definition) -> Option<ViewType> {
    VIEW_TYPES.with_borrow(|types| types.get(definition.as_bytes()).cloned())
}

fn is_element(ty: &convert::Type) -> bool {
    matches!(ty, convert::Type::Nominal { definition, .. }
        if *definition == crate::surface::identity::Definition::element())
}

fn is_view(ty: &convert::Type) -> bool {
    matches!(ty, convert::Type::Nominal { definition, .. } if view_type(definition).is_some())
}

/// `ty` spelled at the view lifetime `'a`, as a recipe family names it.
fn view_text(ty: &convert::Type) -> Option<String> {
    use crate::surface::identity::Definition;
    match ty {
        convert::Type::Nominal { definition, .. } if *definition == Definition::element() => {
            Some(format!("iced::Element<'a, {MESSAGE}>"))
        }
        convert::Type::Nominal { definition, .. } if *definition == Definition::theme() => {
            Some("iced::Theme".to_owned())
        }
        convert::Type::Nominal { definition, .. } if *definition == Definition::renderer() => {
            Some("iced::Renderer".to_owned())
        }
        convert::Type::Nominal {
            definition,
            arguments,
        } if let Some(view) = view_type(definition) => {
            let path = TYPE_PATHS.with_borrow(|paths| paths.get(definition.as_bytes()).cloned())?;
            let mut parts = vec!["'a".to_owned(); view.lifetimes as usize];
            for (index, argument) in arguments.iter().enumerate() {
                // A view's messages are the application's, stamped by the
                // context it lowers in.
                if view.parameters.get(index).map(String::as_str) == Some("Message") {
                    parts.push(MESSAGE.to_owned());
                } else {
                    parts.push(view_text(argument)?);
                }
            }
            Some(format!("{path}<{}>", parts.join(", ")))
        }
        convert::Type::Nominal {
            definition,
            arguments,
        } if !arguments.is_empty() => {
            let path = TYPE_PATHS.with_borrow(|paths| paths.get(definition.as_bytes()).cloned())?;
            let arguments = arguments
                .iter()
                .map(view_text)
                .collect::<Option<Vec<_>>>()?;
            Some(format!("{path}<{}>", arguments.join(", ")))
        }
        convert::Type::Option(element) => Some(format!("Option<{}>", view_text(element)?)),
        convert::Type::Borrowed { mutable: false, ty } => Some(format!("&'a {}", view_text(ty)?)),
        ty => concrete_type(ty),
    }
}

/// The recipe family of `ty`, a view type, declared in `rawr_families`.
fn view_family(ty: &convert::Type) -> Option<String> {
    if !is_view(ty) {
        return None;
    }
    let text = view_text(ty)?;
    Some(VIEW_FAMILIES.with_borrow_mut(|families| {
        let next = families.len();
        families
            .entry(text)
            .or_insert_with(|| format!("RawrView{next}"))
            .clone()
    }))
}

/// The recipe families the adapters named, appended to `rawr_families`.
pub(super) fn render_families(output: &mut Vec<u8>) -> Result<(), request::Error> {
    let families = VIEW_FAMILIES.with_borrow(|families| {
        families
            .iter()
            .map(|(text, name)| (text.clone(), name.clone()))
            .collect::<Vec<_>>()
    });
    for (text, name) in families {
        writeln!(
            output,
            "pub(crate) struct {name};\nimpl {RECIPE}::Family for {name} {{\n    type Of<'a> = {text};\n}}\n"
        )
        .map_err(|_| request::Error::Encoding)?;
    }
    // A native value a view borrows lives in the home, which its arena slot
    // reaches (D568).
    if VIEW_TYPES.with_borrow(|types| !types.is_empty()) {
        writeln!(
            output,
            "impl<T: std::any::Any + Send> Homed for {RECIPE}::Native<T> {{
    fn shared(&self, call: &mut dyn FnMut(&dyn std::any::Any)) -> Result<(), Error> {{
        self.read(|value| call(value)).map_err(Error::Home)
    }}
    fn mutable(&self, call: &mut dyn FnMut(&mut dyn std::any::Any)) -> Result<(), Error> {{
        self.write(|value| call(value)).map_err(Error::Home)
    }}
    fn as_any(&self) -> &dyn std::any::Any {{
        self
    }}
}}
"
        )
        .map_err(|_| request::Error::Encoding)?;
    }
    Ok(())
}

/// What one call of an imported item is in an iced runner: its signature,
/// substitutions and receiver, when it makes or takes a view value.
struct Call<'a> {
    signature: &'a crate::classify::representability::signature::Signature,
    substitutions: &'a [convert::Type],
    constants: &'a [convert::Constant],
    container_parameters: usize,
    receiver: Option<Receiver>,
    callable: Option<&'a crate::classify::representability::callable::Call>,
    associated: bool,
}

fn call(plan: &Plan) -> Option<Call<'_>> {
    match plan.operation() {
        Operation::Function {
            signature,
            substitutions,
            constants,
            functions,
            mappers,
            container_parameters,
            call,
            associated,
            ..
        } if functions.is_empty() && mappers.is_empty() => Some(Call {
            signature,
            substitutions,
            constants,
            container_parameters: *container_parameters,
            receiver: None,
            callable: call.as_ref(),
            associated: *associated,
        }),
        Operation::Method {
            receiver,
            signature,
            substitutions,
            constants,
            container_parameters,
            call,
            ..
        } => Some(Call {
            signature,
            substitutions,
            constants,
            container_parameters: *container_parameters,
            receiver: Some(*receiver),
            callable: call.as_ref(),
            associated: false,
        }),
        _ => None,
    }
}

/// The type a method's receiver has, at the call's instance.
fn container(plan: &Plan, call: &Call<'_>) -> Result<Option<convert::Type>, request::Error> {
    let partition = partition_generics(
        call.signature,
        call.substitutions,
        call.constants,
        call.container_parameters,
    )?;
    let mut types = partition
        .container_parameters
        .iter()
        .filter(|parameter| {
            parameter.kind() == crate::classify::representability::signature::Kind::Type
        })
        .map(|parameter| parameter.index())
        .zip(partition.container_types.iter());
    if let Some(instance) = call.signature.instance() {
        return Ok(super::instance_substituted(instance, &mut types));
    }
    let Some(identity) =
        PATH_IDENTITIES.with_borrow(|identities| identities.get(&rust_path(plan)).copied())
    else {
        return Ok(None);
    };
    Ok(Some(convert::Type::Nominal {
        definition: crate::surface::identity::Definition::authenticated(identity),
        arguments: partition.container_types.to_vec().into_boxed_slice(),
    }))
}

/// Whether `plan`'s adapter makes or takes a view value, so it keeps a
/// recipe.
pub(super) fn is_view_call(plan: &Plan) -> Result<bool, request::Error> {
    if VIEW_TYPES.with_borrow(std::collections::BTreeMap::is_empty) {
        return Ok(false);
    }
    let Some(call) = call(plan) else {
        return Ok(false);
    };
    if is_view(&substituted(
        call.signature.output(),
        call.signature,
        call.substitutions,
    )?) {
        return Ok(true);
    }
    if call.receiver.is_some() && container(plan, &call)?.is_some_and(|ty| is_view(&ty)) {
        return Ok(true);
    }
    for input in call.signature.inputs() {
        if call
            .callable
            .is_some_and(|callable| *input == convert::Type::Parameter(callable.parameter()))
        {
            continue;
        }
        let ty = substituted(input, call.signature, call.substitutions)?;
        if is_view(&ty) || is_element(&ty) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// How an operand of a view call's recipe is taken at the call and given
/// at each replay.
enum Operand {
    /// Cloned into each replay.
    Value,
    /// A view value's recipe, run.
    Recipe,
    /// An iced widget the script passed, lowered.
    Element,
    /// A native value the call borrows, kept in the home.
    Borrowed,
    /// A closure that gives a message when iced calls it.
    Event,
    /// A closure the native constructor calls while it makes the value,
    /// run at the call.
    Made,
    /// A style function of the theme, which iced calls when it draws: run
    /// once each lowering with the view's theme, as Rawr runs a style
    /// function (D492).
    Style,
}

/// Whether `ty` is iced's `Theme`, or a reference to it.
fn is_theme(ty: &convert::Type) -> bool {
    match ty {
        convert::Type::Borrowed { ty, .. } => is_theme(ty),
        convert::Type::Nominal { definition, .. } => {
            *definition == crate::surface::identity::Definition::theme()
        }
        _ => false,
    }
}

fn drift(field: &'static str) -> request::Error {
    request::Error::Drift { field }
}

/// The body of a view call's adapter: it takes its operands, makes the
/// recipe and gives the view value.
pub(super) fn render(plan: &Plan, output: &mut Vec<u8>) -> Result<(), request::Error> {
    let encoding = |_| request::Error::Encoding;
    let call = call(plan).ok_or(drift("a view call's operation"))?;
    let signature = call.signature;
    let substitutions = call.substitutions;
    let returned = substituted(signature.output(), signature, substitutions)?;
    let family = view_family(&returned).ok_or(drift("a view call's output family"))?;
    let widget = match &returned {
        convert::Type::Nominal { definition, .. } => {
            view_type(definition).is_some_and(|view| view.widget)
        }
        _ => false,
    };
    let offset = usize::from(call.receiver.is_some());
    if let Some(receiver) = call.receiver {
        if !matches!(receiver, Receiver::Owned) {
            return Err(drift("a view value's method that borrows it"));
        }
        let container = container(plan, &call)?.ok_or(drift("a view method's receiver"))?;
        let receiver_family =
            view_family(&container).ok_or(drift("a view method's receiver family"))?;
        writeln!(
            output,
            "    let receiver: {RECIPE}::View = take_native(arguments, 0)?;\n    let receiver = receiver.recipe::<{receiver_family}>().ok_or(Error::TypeMismatch)?;"
        )
        .map_err(encoding)?;
    }
    let mut operands = Vec::new();
    for (position, input) in signature.inputs().iter().enumerate() {
        let index = position + offset;
        let closure = call
            .callable
            .filter(|callable| *input == convert::Type::Parameter(callable.parameter()));
        let ty = match closure {
            Some(_) => convert::Type::Unit,
            None => substituted(input, signature, substitutions)?,
        };
        if let Some(callable) = closure {
            let made = substituted(callable.output(), signature, substitutions)?;
            // What native code gives the closure is attested as the
            // closure's parameters, as a script value of its type.
            writeln!(
                output,
                "    let argument_{position}_witnesses = std::sync::Arc::new(closure_inputs({index}, {}));\n    let argument_{position} = std::sync::Arc::new(__rawr_internal::future::Callable::new((**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take())\n        .map_err(|error| Error::Value(__rawr_internal::future::message(&error)))?);",
                callable.inputs().len()
            )
            .map_err(encoding)?;
            let themed = callable
                .inputs()
                .iter()
                .map(|input| substituted(input, signature, substitutions))
                .collect::<Result<Vec<_>, _>>()?
                .iter()
                .any(is_theme);
            operands.push(if is_view(&made) {
                Operand::Made
            } else if themed {
                Operand::Style
            } else {
                Operand::Event
            });
        } else if is_element(&ty) {
            writeln!(
                output,
                "    let argument_{position} = __rawr_profile_host::widget::Value::from_widget((**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take())\n        .map_err(|error| Error::Value(format!(\"{{error:?}}\")))?;"
            )
            .map_err(encoding)?;
            operands.push(Operand::Element);
        } else if let Some(input_family) = view_family(&ty) {
            writeln!(
                output,
                "    let argument_{position}: {RECIPE}::View = take_native(arguments, {index})?;\n    let argument_{position} = argument_{position}.recipe::<{input_family}>().ok_or(Error::TypeMismatch)?;"
            )
            .map_err(encoding)?;
            operands.push(Operand::Recipe);
        } else if let convert::Type::Borrowed {
            mutable: false,
            ty: target,
        } = &ty
            && matches!(**target, convert::Type::Nominal { .. })
        {
            let rust = concrete_type(target).ok_or(drift("a view call's borrowed input"))?;
            writeln!(
                output,
                "    let argument_{position}: {RECIPE}::Native<{rust}> = {{ let handle = native_handle(arguments, {index}, false)?; arguments.arena.home::<{rust}, {RECIPE}::Native<{rust}>>(handle, |value| {RECIPE}::Native::keep(value).map_err(Error::Home))? }};"
            )
            .map_err(encoding)?;
            operands.push(Operand::Borrowed);
        } else {
            render_take_input(
                position,
                index,
                input,
                signature,
                substitutions,
                None,
                output,
            )?;
            operands.push(Operand::Value);
        }
    }
    // The native constructor's closures that make view values run now, in
    // a dry run over the copies the call borrows.
    let made = operands
        .iter()
        .position(|operand| matches!(operand, Operand::Made));
    let path = call_path(plan, &call)?;
    if let Some(made_position) = made {
        let callable = call.callable.ok_or(drift("a view call's closure"))?;
        let made_family = view_family(&substituted(callable.output(), signature, substitutions)?)
            .ok_or(drift("a view closure's output family"))?;
        let inputs = callable
            .inputs()
            .iter()
            .map(|input| substituted(input, signature, substitutions))
            .collect::<Result<Vec<_>, _>>()?;
        let mut parameters = Vec::new();
        let mut values = Vec::new();
        for (index, input) in inputs.iter().enumerate() {
            parameters.push(format!("input_{index}"));
            let mut converted = String::new();
            render_to_dynamic(input, &format!("input_{index}"), 0, &mut converted)?;
            values.push(format!(
                "{{ let value = ({converted})?; attest_read(&mut *arguments.arena, value, &argument_{made_position}_witnesses[{index}])? }}"
            ));
        }
        let dry_arguments = operands
            .iter()
            .enumerate()
            .map(|(position, operand)| match operand {
                Operand::Made => format!(
                    "|{}| {{
            let made = (|| -> Result<{RECIPE}::Recipe<{made_family}>, Error> {{
                let mut values = with_arena(|arena| {{ let mut none: [&mut Dynamic; 0] = []; let arguments = &mut Arguments {{ values: &mut none, arena }}; let _ = &arguments; Ok::<_, Error>([{}]) }})?;
                let value = argument_{made_position}.call(&mut values).map_err(|error| Error::Value(__rawr_internal::future::message(&error)))?;
                let view: {RECIPE}::View = with_arena(|arena| {{ let handle = value.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch)?; arena.take::<{RECIPE}::View>(handle) }})?;
                view.recipe::<{made_family}>().ok_or(Error::TypeMismatch)
            }})();
            match made {{
                Ok(recipe) => {{
                    let value = recipe.run(&dry);
                    recorded.borrow_mut().push(recipe);
                    value
                }}
                Err(error) => {{
                    *failure.borrow_mut() = Some(error);
                    panic!(\"rawr: a view closure failed\")
                }}
            }}
        }}",
                    parameters.join(", "),
                    values.join(", ")
                ),
                Operand::Borrowed => format!("argument_{position}_borrowed"),
                _ => format!("Clone::clone(&argument_{position})"),
            })
            .collect::<Vec<_>>();
        if dry_arguments.len() != operands.len()
            || operands.iter().any(|operand| {
                !matches!(operand, Operand::Made | Operand::Borrowed | Operand::Value)
            })
            || call.receiver.is_some()
        {
            return Err(drift("a view constructor's operands"));
        }
        // The dry run borrows each native value the call borrows where it
        // lives, in the home.
        let mut ran = format!(
            "lend(arguments.arena, || std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {{
        let _ = {path}({});
    }})))",
            dry_arguments.join(", ")
        );
        for (position, operand) in operands.iter().enumerate().rev() {
            if matches!(operand, Operand::Borrowed) {
                ran = format!(
                    "argument_{position}.read(|argument_{position}_borrowed| {ran}).map_err(Error::Home)?"
                );
            }
        }
        writeln!(
            output,
            "    let dry = {RECIPE}::dry_context();
    let recorded = std::cell::RefCell::new(Vec::new());
    let failure = std::cell::RefCell::new(None::<Error>);
    let ran = {ran};
    if let Some(error) = failure.into_inner() {{
        return Err(error);
    }}
    if let Err(panic) = ran {{
        std::panic::resume_unwind(panic);
    }}
    let argument_{made_position} = std::sync::Arc::new(recorded.into_inner());"
        )
        .map_err(encoding)?;
    }
    let replayed = operands
        .iter()
        .enumerate()
        .map(|(position, operand)| match operand {
            Operand::Value => format!("Clone::clone(&argument_{position})"),
            Operand::Recipe => format!("argument_{position}.run(context)"),
            Operand::Element => format!("argument_{position}.lower(context)"),
            Operand::Borrowed => format!(
                "argument_{position}.get(context).expect(\"rawr: the view's home holds what the view borrows\")"
            ),
            Operand::Event => {
                let inputs = call
                    .callable
                    .map(|callable| callable.inputs().len())
                    .unwrap_or_default();
                let parameters = (0..inputs)
                    .map(|index| format!("input_{index}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                event_closure(position, &parameters, call.callable, signature, substitutions)
                    .unwrap_or_default()
            }
            Operand::Style => {
                let callable = call.callable;
                let output = callable
                    .and_then(|callable| substituted(callable.output(), signature, substitutions).ok())
                    .and_then(|output| concrete_type(&output))
                    .unwrap_or_default();
                let ignored = callable
                    .map(|callable| vec!["_"; callable.inputs().len()].join(", "))
                    .unwrap_or_default();
                format!(
                    "{{ let style: {output} = {{ let mut values = [Dynamic::from(context.theme().clone())]; let value = argument_{position}.call(&mut values).unwrap_or_else(|error| panic!(\"{{}}\", __rawr_internal::future::message(&error))); value.try_cast::<{output}>().expect(\"rawr: a style function gives its style\") }}; move |{ignored}| Clone::clone(&style) }}"
                )
            }
            Operand::Made => {
                let ignored = call
                    .callable
                    .map(|callable| {
                        callable
                            .inputs()
                            .iter()
                            .map(|input| match input {
                                convert::Type::Borrowed { .. } => "_: &_",
                                _ => "_",
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                format!(
                    "{{ let next = std::cell::Cell::new(0_usize); let made = &argument_{position}; move |{ignored}| {{ let index = next.get(); next.set(index + 1); made.get(index).or_else(|| made.last()).expect(\"rawr: the view's closure made a value at its transition\").run(context) }} }}"
                )
            }
        })
        .collect::<Vec<_>>();
    let body = match call.receiver {
        Some(_) => format!(
            "receiver.run(context).{}({})",
            plan.name(),
            replayed.join(", ")
        ),
        None => format!("{path}({})", replayed.join(", ")),
    };
    writeln!(
        output,
        "    let recipe = {RECIPE}::Recipe::<{family}>::new(move |context| {body});"
    )
    .map_err(encoding)?;
    if widget {
        writeln!(
            output,
            "    let view = {RECIPE}::View::widget(recipe, |recipe| recipe.then::<{RECIPE}::ElementFamily>(|widget, _| iced::advanced::Widget::_boxed(widget)));"
        )
        .map_err(encoding)?;
    } else {
        writeln!(output, "    let view = {RECIPE}::View::part(recipe);").map_err(encoding)?;
    }
    writeln!(
        output,
        "    insert_native!(arguments.arena, view).map(Dynamic::from)"
    )
    .map_err(encoding)
}

/// The native closure a view gives iced for an event closure the script
/// passed: it runs the script closure when iced calls it, and stamps the
/// message it gives for the application the view lowers for.
fn event_closure(
    position: usize,
    parameters: &str,
    callable: Option<&crate::classify::representability::callable::Call>,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
) -> Option<String> {
    let callable = callable?;
    let inputs = callable
        .inputs()
        .iter()
        .map(|input| substituted(input, signature, substitutions).ok())
        .collect::<Option<Vec<_>>>()?;
    let mut values = Vec::new();
    for (index, input) in inputs.iter().enumerate() {
        let mut converted = String::new();
        render_to_dynamic(input, &format!("input_{index}"), 0, &mut converted).ok()?;
        values.push(format!(
            "{{ let value = ({converted})?; attest_read(&mut *arguments.arena, value, &witnesses[{index}])? }}"
        ));
    }
    Some(format!(
        "{{ let callable = std::sync::Arc::clone(&argument_{position}); let witnesses = std::sync::Arc::clone(&argument_{position}_witnesses); let context = context.clone(); move |{parameters}| {{ let mut values = with_arena(|arena| {{ let mut none: [&mut Dynamic; 0] = []; let arguments = &mut Arguments {{ values: &mut none, arena }}; let _ = &arguments; Ok::<_, Error>([{}]) }}).unwrap_or_else(|error| panic!(\"rawr: a closure's input can't cross: {{error:?}}\")); let message = callable.call(&mut values).unwrap_or_else(|error| panic!(\"{{}}\", __rawr_internal::future::message(&error))); {RECIPE}::message(&context, message) }} }}",
        values.join(", ")
    ))
}

/// The path a view call's function is reached at: an associated function
/// through its type's own path, as `iced::widget::pane_grid::Content::new`,
/// without generic arguments, which the recipe's family settles.
fn call_path(plan: &Plan, call: &Call<'_>) -> Result<String, request::Error> {
    if call.receiver.is_some() {
        return Ok(String::new());
    }
    if call.associated {
        return Ok(format!("{}::{}", rust_path(plan), plan.name()));
    }
    if call.container_parameters != 0 {
        return Err(drift(
            "a view call's free function has container parameters",
        ));
    }
    Ok(rust_path(plan))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::identity::Definition;

    /// A view type's family names it at the view lifetime, its `Message`
    /// the application's, and its `Theme` and `Renderer` iced's.
    #[test]
    fn a_view_type_is_spelled_at_the_view_lifetime() {
        TYPE_PATHS.with_borrow_mut(|paths| {
            paths.insert([7; 32], "iced::widget::PaneGrid".to_owned());
        });
        VIEW_TYPES.with_borrow_mut(|types| {
            types.insert(
                [7; 32],
                ViewType {
                    lifetimes: 1,
                    widget: true,
                    parameters: ["Message", "Theme", "Renderer"].map(str::to_owned).to_vec(),
                },
            );
        });
        let nominal = |definition| convert::Type::Nominal {
            definition,
            arguments: Box::default(),
        };
        let ty = convert::Type::Nominal {
            definition: Definition::authenticated([7; 32]),
            arguments: Box::new([
                nominal(Definition::element()),
                nominal(Definition::theme()),
                nominal(Definition::renderer()),
            ]),
        };
        assert_eq!(
            view_text(&ty).as_deref(),
            Some(
                "iced::widget::PaneGrid<'a, __rawr_profile_host::runtime::Message, iced::Theme, iced::Renderer>"
            )
        );
        assert!(view_family(&ty).is_some());
        clear();
        TYPE_PATHS.with_borrow_mut(std::collections::BTreeMap::clear);
        assert!(!is_view(&ty));
    }
}
