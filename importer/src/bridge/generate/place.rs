//! Concrete native place descriptors, shared by all callbacks in a runner.

use super::{concrete_type, convert, request};
use std::collections::BTreeMap;

thread_local! {
    static POLICIES: std::cell::RefCell<BTreeMap<convert::Type, (String, String)>> =
        const { std::cell::RefCell::new(BTreeMap::new()) };
    static VECTORS: std::cell::RefCell<BTreeMap<convert::Type, String>> =
        const { std::cell::RefCell::new(BTreeMap::new()) };
}

pub(super) fn clear() {
    POLICIES.with_borrow_mut(BTreeMap::clear);
    VECTORS.with_borrow_mut(BTreeMap::clear);
}

pub(super) fn render(output: &mut Vec<u8>) {
    POLICIES.with_borrow(|policies| {
        for (_, source) in policies.values() {
            output.extend_from_slice(source.as_bytes());
        }
    });
    output.extend_from_slice(b"fn register_native_sequences() {\n");
    POLICIES.with_borrow(|policies| {
        for ty in policies.keys() {
            let rust = concrete_type(ty).expect("a native descriptor has a concrete type");
            output.extend_from_slice(reading(&rust).as_bytes());
        }
    });
    VECTORS.with_borrow(|vectors| {
        for (ty, element) in vectors {
            let key = serde_json::to_vec(ty).expect("a concrete conversion type serializes");
            let rust = concrete_type(ty).expect("a native sequence descriptor has a concrete type");
            output.extend_from_slice(reading(&rust).as_bytes());
            output.extend_from_slice(format!(
                "    __rawr_internal::place::sequence::register::<{element}>(&{key:?}).expect(\"unique generated native sequence descriptor\");
    __rawr_internal::place::sequence::register_capabilities::<{rust}>(
        (&DebugProbe::<{rust}>(std::marker::PhantomData)).native_sequence_cloning(),
        (&DebugProbe::<{rust}>(std::marker::PhantomData)).native_sequence_copying(),
        (&DebugProbe::<{rust}>(std::marker::PhantomData)).native_sequence_comparing(),
    );\n"
            ).as_bytes());
        }
    });
    output.extend_from_slice(b"}\n");
}

fn reading(rust: &str) -> String {
    format!("    __rawr_internal::place::reading::register::<{rust}>(__rawr_internal::place::reading::Reading::from_traits(
        (&DebugProbe::<{rust}>(std::marker::PhantomData)).native_place_debugging(),
        (&DebugProbe::<{rust}>(std::marker::PhantomData)).native_place_displaying(),
        (&DebugProbe::<{rust}>(std::marker::PhantomData)).native_place_partial(),
        (&DebugProbe::<{rust}>(std::marker::PhantomData)).native_place_total(),
    ));\n")
}

fn arena(expression: &str) -> String {
    format!(
        "with_arena(|arena| {{ let mut none: [&mut Dynamic; 0] = []; let arguments = &mut Arguments {{ values: &mut none, arena }}; {expression} }}).map_err(|error| -> Box<__rawr_internal::EvalAltResult> {{ __rawr_internal::EvalAltResult::ErrorRuntime(format!(\"native place conversion: {{error:?}}\").into(), __rawr_internal::Position::NONE).into() }})"
    )
}

/// Whether this concrete descriptor is available. Other already supported
/// owned inputs retain their existing codec while their place policies are
/// added; installing native ownership must not narrow ordinary call admission.
pub(super) fn available(ty: &convert::Type) -> bool {
    match ty {
        convert::Type::Builtin(builtin) => {
            *builtin == convert::Builtin::String || builtin.rust_name().is_some()
        }
        convert::Type::Sequence {
            element,
            owned: true,
            length: None,
        } => available(element),
        convert::Type::Nominal { arguments, .. } => {
            !super::carries_lifetime(ty) && arguments.iter().all(available)
        }
        _ => false,
    }
}

pub(super) fn around(
    places: &[(usize, bool, String)],
    witnesses: &str,
    mut call: String,
) -> String {
    if places.is_empty() {
        return call;
    }
    for (position, mutable, policy) in places.iter().rev() {
        let borrow = if *mutable { "borrow" } else { "borrow_ref" };
        call = format!(
            "__rawr_internal::place::{borrow}::<{policy}, _>(input_{position}, place_witnesses.get({position}).and_then(Option::as_ref).expect(\"checked native sequence parameter witness\"), |place_{position}| {{ {call} }}).unwrap_or_else(|error| panic!(\"rawr: native place: {{error}}\"))"
        );
    }
    format!("let place_witnesses = {witnesses};\n        {call}")
}

/// The descriptor for an exact native value, with generated codecs only for
/// real ownership transfers. Merely lending its storage invokes neither codec.
pub(super) fn policy(ty: &convert::Type) -> Result<String, request::Error> {
    let drift = || request::Error::Drift {
        field: "a native place descriptor's concrete type",
    };
    match ty {
        convert::Type::Builtin(convert::Builtin::String) => {
            return Ok("__rawr_internal::place::text::Text".into());
        }
        convert::Type::Sequence {
            element,
            owned: true,
            length: None,
        } => {
            let element = policy(element)?;
            VECTORS.with_borrow_mut(|vectors| vectors.insert(ty.clone(), element.clone()));
            return Ok(format!(
                "__rawr_internal::place::sequence::Vector<{element}>"
            ));
        }
        _ => {}
    }
    if let Some(name) =
        POLICIES.with_borrow(|policies| policies.get(ty).map(|(name, _)| name.clone()))
    {
        return Ok(name);
    }
    let rust = concrete_type(ty).ok_or_else(drift)?;
    let (accepts, copy, nominal) = match ty {
        convert::Type::Builtin(builtin) if builtin.rust_name().is_some() => {
            let builtin = match builtin {
                convert::Builtin::Bool => "bool",
                convert::Builtin::Char => "char",
                convert::Builtin::F32 | convert::Builtin::F64 => "f64",
                _ => "i64",
            };
            (format!("witness.is_builtin({builtin:?})"), true, false)
        }
        convert::Type::Nominal {
            definition,
            arguments,
        } if !super::carries_lifetime(ty) => {
            let mut accepts = format!("witness.is_imported(&{:?})", definition.as_bytes());
            for (at, argument) in arguments.iter().enumerate() {
                let descriptor = policy(argument)?;
                accepts.push_str(&format!(
                    " && witness.argument_witness({at}).is_some_and(|witness| <{descriptor} as __rawr_internal::place::Policy>::accepts(&witness))"
                ));
            }
            let copy = super::COPY_TYPES.with_borrow(|types| types.contains(definition.as_bytes()));
            (accepts, copy, true)
        }
        _ => return Err(drift()),
    };
    let name = POLICIES.with_borrow(|policies| format!("NativePlace{}", policies.len()));
    let mut decoded = String::new();
    super::render_from_dynamic(ty, "value", 0, &mut decoded)?;
    let decoded = arena(&decoded);
    let mut encoded = String::new();
    super::render_to_dynamic(ty, "value", 0, &mut encoded)?;
    let encoded = arena(&format!(
        "let value = ({encoded})?; attest_nested(arguments.arena, value, witness)"
    ));
    let copied = if copy {
        "Self::into_dynamic(*value.read(), witness)".to_owned()
    } else {
        "Err(__rawr_internal::EvalAltResult::ErrorRuntime(\"the native element does not implement Copy\".into(), __rawr_internal::Position::NONE).into())".to_owned()
    };
    let copy_operand = if !copy {
        "Err(__rawr_internal::EvalAltResult::ErrorRuntime(\"the native element does not implement Copy\".into(), __rawr_internal::Position::NONE).into())".to_owned()
    } else if nominal {
        arena(&format!("let handle = value.clone().try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch)?; arguments.arena.shared::<{rust}, _>(handle, |value| *value)"))
    } else {
        "Self::from_dynamic(value.clone(), witness)".to_owned()
    };
    let borrowed = if nominal {
        format!("with_arena(|arena| {{ let handle = value.clone().try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch)?; arena.shared::<{rust}, _>(handle, |value| visit(value)) }}).map_err(place_error)?")
    } else {
        "let value = Self::from_dynamic(value.clone(), witness)?; visit(&value)".to_owned()
    };
    let duplicated = arena("guard_copy!(value.read())");
    let lending = if nominal {
        format!(
            "
    fn lend(value: &mut {rust}, _witness: &__rawr_internal::host::runtime::Witness, rest: &mut __rawr_internal::scope::Rest<'_>) -> Result<Dynamic, Box<__rawr_internal::EvalAltResult>> {{
        __rawr_internal::scope::lend::<Self, _>(value, |nonce| rest(Dynamic::from(__rawr_internal::scope::Lent::new(nonce, {rust:?}, Some(&Self::MUTABLE), Some(&Self::SHARED)))))
    }}
    fn lend_ref(value: &{rust}, _witness: &__rawr_internal::host::runtime::Witness, rest: &mut __rawr_internal::scope::Rest<'_>) -> Result<Dynamic, Box<__rawr_internal::EvalAltResult>> {{
        __rawr_internal::scope::lend_ref::<Self, _>(value, |nonce| rest(Dynamic::from(__rawr_internal::scope::Lent::new(nonce, {rust:?}, None, Some(&Self::SHARED)))))
    }}
"
        )
    } else {
        String::new()
    };
    let family = if nominal {
        format!(
            "
impl __rawr_internal::scope::Family for {name} {{ type Of<'a> = {rust}; }}
impl {name} {{
    const SHARED: fn(u64, &mut dyn FnMut(&{rust})) -> Result<(), u8> = |nonce, visit| __rawr_internal::scope::with_ref::<Self, _>(__rawr_internal::scope::nonce(nonce), |value| visit(value)).map_err(__rawr_internal::scope::Error::code);
    const MUTABLE: fn(u64, &mut dyn FnMut(&mut {rust})) -> Result<(), u8> = |nonce, visit| __rawr_internal::scope::with::<Self, _>(__rawr_internal::scope::nonce(nonce), |value| visit(value)).map_err(__rawr_internal::scope::Error::code);
}}
"
        )
    } else {
        String::new()
    };
    let owner = if nominal {
        format!("PlaceOwner<{rust}>")
    } else {
        "__rawr_internal::place::ownership::Native".to_owned()
    };
    let source = format!(
        "
#[allow(dead_code)]
struct {name};
#[allow(clippy::all, unused_variables)]
impl __rawr_internal::place::Policy for {name} {{
    type Value = {rust};
    type Owner = {owner};
    fn accepts(witness: &__rawr_internal::host::runtime::Witness) -> bool {{ {accepts} }}
    fn from_dynamic(value: Dynamic, witness: &__rawr_internal::host::runtime::Witness) -> Result<{rust}, Box<__rawr_internal::EvalAltResult>> {{
        if let Some(place) = __rawr_internal::place::get(&value) {{ return __rawr_internal::place::take::<Self>(&place, witness); }}
        {decoded}
    }}
    fn into_dynamic(value: {rust}, witness: &__rawr_internal::host::runtime::Witness) -> Result<Dynamic, Box<__rawr_internal::EvalAltResult>> {{ {encoded} }}
    fn cloning() -> Option<&'static __rawr_internal::place::sequence::Cloning<{rust}>> {{
        static CLONING: std::sync::LazyLock<Option<__rawr_internal::place::sequence::Cloning<{rust}>>> = std::sync::LazyLock::new(|| {{
            use crate::{{ViaSequenceClone as _, WithoutSequenceClone as _}};
            (&DebugProbe::<{rust}>(std::marker::PhantomData)).native_sequence_cloning()
        }});
        CLONING.as_ref()
    }}
    fn copying() -> Option<&'static __rawr_internal::place::sequence::Copying<{rust}>> {{
        static COPYING: std::sync::LazyLock<Option<__rawr_internal::place::sequence::Copying<{rust}>>> = std::sync::LazyLock::new(|| {{
            use crate::{{ViaSequenceCopy as _, WithoutSequenceCopy as _}};
            (&DebugProbe::<{rust}>(std::marker::PhantomData)).native_sequence_copying()
        }});
        COPYING.as_ref()
    }}
    fn comparing() -> Option<&'static __rawr_internal::place::sequence::Comparing<{rust}>> {{
        static COMPARING: std::sync::LazyLock<Option<__rawr_internal::place::sequence::Comparing<{rust}>>> = std::sync::LazyLock::new(|| {{
            use crate::{{ViaSequenceEqual as _, WithoutSequenceEqual as _}};
            (&DebugProbe::<{rust}>(std::marker::PhantomData)).native_sequence_comparing()
        }});
        COMPARING.as_ref()
    }}
    fn with_dynamic<R>(value: &Dynamic, witness: &__rawr_internal::host::runtime::Witness, visit: impl FnOnce(&{rust}) -> Result<R, Box<__rawr_internal::EvalAltResult>>) -> Result<R, Box<__rawr_internal::EvalAltResult>> {{
        if let Some(place) = __rawr_internal::place::get(value) {{
            return __rawr_internal::place::reach::<Self, _>(&place, witness, false, |value| visit(value.read()));
        }}
        {borrowed}
    }}
    fn copy_dynamic(value: &Dynamic, witness: &__rawr_internal::host::runtime::Witness) -> Result<{rust}, Box<__rawr_internal::EvalAltResult>> {{
        if let Some(place) = __rawr_internal::place::get(value) {{
            let value = place.invoke(__rawr_internal::place::Operation::Copy, &mut [])?;
            return Self::from_dynamic(value, witness);
        }}
        {copy_operand}
    }}
    fn invoke(mut value: __rawr_internal::place::Access<'_, {rust}>, witness: &__rawr_internal::host::runtime::Witness, operation: __rawr_internal::place::Operation, arguments: &mut [Dynamic], _rest: Option<&mut __rawr_internal::scope::Rest<'_>>) -> Result<Dynamic, Box<__rawr_internal::EvalAltResult>> {{
        match operation {{
            __rawr_internal::place::Operation::Copy => {{ {copied} }}
            __rawr_internal::place::Operation::Clone => {{ let copied = ({duplicated})?; Self::into_dynamic(copied, witness) }}
            __rawr_internal::place::Operation::Store => {{
                let incoming = arguments.get_mut(0).ok_or_else(|| __rawr_internal::EvalAltResult::ErrorRuntime(\"missing native store operand\".into(), __rawr_internal::Position::NONE))?.take();
                let incoming = Self::from_dynamic(incoming, witness)?;
                *value.write()? = incoming;
                Ok(Dynamic::UNIT)
            }}
            _ => __rawr_internal::place::reading::invoke::<Self>(value, witness, operation, arguments),
        }}
    }}{lending}
}}
{family}
"
    );
    POLICIES.with_borrow_mut(|policies| policies.insert(ty.clone(), (name.clone(), source)));
    Ok(name)
}
