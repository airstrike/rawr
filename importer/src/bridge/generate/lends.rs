//! A call whose output keeps borrowed the slices of crate values the
//! program lends it from places (D558 tier 1, `Scope::Lends`), as iced's
//! `layout.iter_mut(&mut tree.children)` gives an iterator of `(Layout,
//! &mut Tree)`. The call opens a scope frame that reaches each lent slice
//! where it is, makes the output through a wrapper whose family its region
//! holds it by, and runs the rest of the scope while the lends last. Each
//! item's borrow of an element is lent to the program from that region
//! (`scope::Region::lend`).

use std::io::Write as _;

use super::{
    concrete_type, convert, family, holds_opaque, render_container_path, render_take_input,
    request, rust_path, scoped_receiver_call, scoped_type, substituted, Operation, Plan, Receiver,
};

/// Declare the wrapper of the method `plan`, whose output, of an opaque
/// type, keeps the slices it takes borrowed (`Scope::Lends`), as iced's
/// `Layout::iter_mut(self, &mut [Tree])` does, and give the output's type
/// when the declaration is new: its inputs are the receiver, each slice at
/// `'x` and each other argument, and its bounds the opaque type's. None for
/// a function, or when an input has no Rust spelling.
pub(super) fn declare_lent_wrapper(
    plan: &Plan,
    returned: convert::Type,
) -> Result<Option<convert::Type>, request::Error> {
    let Operation::Method {
        receiver: receiver @ Receiver::Owned,
        signature,
        substitutions,
        constants,
        container_parameters,
        ..
    } = plan.operation()
    else {
        return Ok(None);
    };
    let Some(key) = scoped_type(&returned) else {
        return Ok(None);
    };
    if family::of(&key).is_some() {
        return Ok(None);
    }
    let Some(bounds) = lent_bounds(signature, &returned) else {
        return Ok(None);
    };
    let path = rust_path(plan);
    let mut container = Vec::new();
    render_container_path(
        &path,
        signature,
        substitutions,
        constants,
        *container_parameters,
        true,
        &mut container,
    )?;
    let container = String::from_utf8(container).map_err(request::unwritable)?;
    let mut inputs = vec![("receiver".to_owned(), container)];
    for (position, input) in signature.inputs().iter().enumerate() {
        let input = substituted(input, signature, substitutions)?;
        let ty = match lends_slice(&input) {
            Some((mutable, element)) => concrete_type(element).map(|element| {
                let mutable = if mutable { "mut " } else { "" };
                format!("&'x {mutable}[{element}]")
            }),
            None => scoped_type(&input).filter(|_| !holds_opaque(&input)),
        };
        let Some(ty) = ty else {
            return Ok(None);
        };
        inputs.push((format!("argument_{position}"), ty));
    }
    let call = scoped_receiver_call(
        plan,
        *receiver,
        signature,
        substitutions,
        constants,
        *container_parameters,
    )?;
    family::Wrapper::new(inputs, call, bounds).declare(key);
    Ok(Some(returned))
}

/// The bounds of the wrapper of a call whose output keeps lent slices
/// borrowed (`declare_lent_wrapper`): core's `Iterator` at the item the
/// opaque type's iterator mark names, spelled at `'x`, as iced's
/// `Layout::iter_mut` gives `impl DoubleEndedIterator<Item = (Self, &mut
/// Tree)> + ExactSizeIterator`. That is the trait whose members the
/// program calls on it (`next`). None for an output that is no iterator,
/// or whose item has no Rust spelling.
fn lent_bounds(
    signature: &crate::classify::representability::signature::Signature,
    returned: &convert::Type,
) -> Option<String> {
    let convert::Type::Nominal {
        definition,
        arguments,
    } = returned
    else {
        return None;
    };
    let opaque = signature
        .opaques()
        .iter()
        .find(|opaque| opaque.definition() == definition.as_bytes())?;
    let mark = *crate::surface::identity::Definition::iterator().as_bytes();
    let (_, position) = opaque
        .associated()
        .iter()
        .find(|(associated, _)| associated.trait_identity() == mark)?;
    let item = scoped_type(arguments.get(usize::try_from(*position).ok()?)?)?;
    Some(format!("::core::iter::Iterator<Item = {item}>"))
}

/// Whether `ty` is a slice of crate values a call borrows, `&mut [T]` or
/// `&[T]`, as iced's `Layout::iter_mut` takes its `&mut [Tree]`: whether
/// it is mutable, and its element.
fn lends_slice(ty: &convert::Type) -> Option<(bool, &convert::Type)> {
    let convert::Type::Borrowed { mutable, ty } = ty else {
        return None;
    };
    let convert::Type::Sequence {
        element,
        owned: false,
        length: None,
    } = ty.as_ref()
    else {
        return None;
    };
    matches!(**element, convert::Type::Nominal { .. }).then_some((*mutable, &**element))
}

/// A method call whose output keeps slices of crate values borrowed that
/// the program lends from places (D558 tier 1, `Scope::Lends`), as iced's
/// `layout.iter_mut(&mut tree.children)`: it gives a scope frame the
/// statement it is in opens. The frame reaches each lent slice where it
/// is, as rustc borrows it, makes the output through its wrapper
/// (`declare_lent_wrapper`), holds it in a region of its own, and runs the
/// rest of the scope while it holds the lends. The output is lazy, as
/// rustc's is: each item is made when the program asks for it.
pub(super) fn render_lends_opening(
    plan: &Plan,
    signature: &crate::classify::representability::signature::Signature,
    substitutions: &[convert::Type],
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    let encoding = request::unwritable;
    let returned = substituted(signature.output(), signature, substitutions)?;
    let family = scoped_type(&returned)
        .and_then(|key| family::of(&key))
        .ok_or_else(|| request::drift("a scope frame's lent output without its wrapper"))?;
    let made = family
        .made()
        .ok_or_else(|| request::drift("a scope frame's lent output without its call"))?;
    let name = format!("{}::{}", rust_path(plan), plan.name());
    writeln!(output, "    let receiver = take_native(arguments, 0)?;").map_err(encoding)?;
    let mut owned = vec!["receiver".to_owned()];
    let mut lent = Vec::new();
    for (position, ty) in signature.inputs().iter().enumerate() {
        let input = substituted(ty, signature, substitutions)?;
        if let Some((mutable, element)) = lends_slice(&input) {
            let element = concrete_type(element)
                .ok_or_else(|| request::drift("a lent slice's element without a Rust type"))?;
            writeln!(
                output,
                "    let lender_{position} = Lender::of(arguments.values.get({}).ok_or(Error::MissingArgument)?).map_err(|_| Error::Value({:?}.to_owned()))?;",
                position + 1,
                format!(
                    "rawr lends `{name}` a slice only from a crate value's field, as `&mut value.field`, yet"
                ),
            )
            .map_err(encoding)?;
            lent.push((position, mutable, element));
        } else {
            render_take_input(
                position,
                position + 1,
                ty,
                signature,
                substitutions,
                None,
                output,
            )?;
            owned.push(format!("argument_{position}"));
        }
    }
    let owned = owned.join(", ");
    let insert = family.call(family::Reach::Insert, "made");
    // In a stand-in's lending region, slices of values native code lent
    // the stand-in kept there are reached there at the region's lifetime,
    // and the output stays there, as `layout.iter_mut(&mut tree.children)`
    // in a program's `overlay` gives items its output may keep (#2698).
    if !lent.is_empty() && lent.iter().all(|(_, mutable, _)| *mutable) {
        let kept = lent
            .iter()
            .map(|(position, ..)| {
                format!(
                    "matches!(&lender_{position}, Lender::Lent(lent) if lent.is_kept_in(anchor))"
                )
            })
            .collect::<Vec<_>>()
            .join(" && ");
        let mut reached = String::new();
        for (position, _, element) in &lent {
            reached.push_str(&format!(
                "
            let Lender::Lent(lent_{position}) = &lender_{position} else {{
                return Err(Error::Unhandled);
            }};
            let argument_{position} = &mut region.lent_mut::<Vec<{element}>>(*lent_{position}).map_err(|error| Error::Lent(lent_{position}.name(), error))?[..];"
            ));
        }
        writeln!(
            output,
            "    if let Some(anchor) = __rawr_internal::scope::anchor().filter(|&anchor| {kept}) {{
        return __rawr_internal::scope::with::<__rawr_internal::scope::RegionF, _>(anchor, |region| -> Result<Dynamic, Error> {{{reached}
            let made = {made};
            Ok(Dynamic::from(region.{insert}))
        }})
        .map_err(|error| Error::Lent(\"a lending call's region\", error))?;
    }}"
        )
        .map_err(encoding)?;
    }
    let mut body = format!(
        "let ({owned},) = taken.take().ok_or(Error::Unhandled).map_err(runtime_error)?;
            let made = {made};
            let mut region = __rawr_internal::scope::Region::new();
            let value = Dynamic::from(region.{insert});
            __rawr_internal::scope::open(&mut region, || rest(value))"
    );
    for (position, mutable, element) in lent.iter().rev() {
        let (reach, borrow) = if *mutable {
            ("reach_lender_mut", "&mut ")
        } else {
            ("reach_lender", "&")
        };
        body = format!(
            "{reach}::<Vec<{element}>>(&lender_{position}, &mut |argument_{position}| {{
            let argument_{position} = {borrow}argument_{position}[..];
            {body}
        }})"
        );
    }
    writeln!(
        output,
        "    let output = __rawr_internal::scope::Opening::new({name:?}, move |rest| {{
        let mut taken = Some(({owned},));
        {body}
    }});
    Ok(Dynamic::from(output))"
    )
    .map_err(encoding)
}
