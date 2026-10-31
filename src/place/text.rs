//! Actual native String storage, including strings projected from vectors.

use super::{argument, failure, get, index, Access, Operation, Policy};
use crate::host::runtime::Witness;
use crate::{Dynamic, ImmutableString, RawrResult, RawrResultOf};

/// The descriptor of native owned UTF-8 text.
pub struct Text;

static CLONING: super::sequence::Cloning<String> = super::sequence::cloning::<String>();
static COMPARING: super::sequence::Comparing<String> = super::sequence::comparing::<String>();

fn read<R>(value: Dynamic, visit: impl FnOnce(&str) -> RawrResultOf<R>) -> RawrResultOf<R> {
    if let Some(place) = get(&value) {
        return super::reach::<Text, _>(&place, place.witness(), false, |value| {
            visit(value.read())
        });
    }
    let text = value
        .read_lock::<ImmutableString>()
        .ok_or_else(|| failure("a text operand is neither text nor a native String place"))?;
    visit(&text)
}

fn boundary(value: &str, at: usize, allow_end: bool) -> RawrResultOf<()> {
    if !value.is_char_boundary(at) || (!allow_end && at == value.len()) {
        return Err(failure(
            "native String index is not a valid character boundary",
        ));
    }
    Ok(())
}

/// Run String's native retain algorithm with the active script callback context.
pub(crate) fn retain_with(
    context: &crate::NativeCallContext<'_>,
    callback: Dynamic,
    run: impl FnOnce(&mut dyn FnMut(char) -> bool),
) -> RawrResult {
    super::callback::within(context, || retain_in_context(callback, run))
}

fn retain_in_context(
    callback: Dynamic,
    run: impl FnOnce(&mut dyn FnMut(char) -> bool),
) -> RawrResult {
    let callable = super::callback::Callable::new(callback)?;
    super::callback::run(|abort| {
        run(&mut |letter| {
            abort.value(callable.call(&mut [letter.into()]).and_then(|value| {
                value
                    .as_bool()
                    .map_err(|_| failure("String::retain predicate did not return bool"))
            }))
        });
    })?;
    Ok(Dynamic::UNIT)
}

impl Policy for Text {
    type Value = String;
    type Owner = super::ownership::Native;

    fn accepts(witness: &Witness) -> bool {
        witness.is_builtin("String")
    }

    fn from_dynamic(value: Dynamic, witness: &Witness) -> RawrResultOf<String> {
        if let Some(place) = get(&value) {
            return super::take::<Self>(&place, witness);
        }
        value
            .try_cast_result::<ImmutableString>()
            .map(|value| value.to_string())
            .map_err(|_| failure("a String operand is neither text nor native ownership"))
    }

    fn into_dynamic(value: String, witness: &Witness) -> RawrResult {
        super::owned::<Self>(value, witness)
    }

    fn cloning() -> Option<&'static super::sequence::Cloning<String>> {
        Some(&CLONING)
    }

    fn clone_dynamic(value: &Dynamic, witness: &Witness) -> RawrResultOf<String> {
        if let Some(place) = get(value) {
            return super::reach::<Self, _>(&place, witness, false, |value| {
                Ok(value.read().clone())
            });
        }
        let value = value
            .read_lock::<ImmutableString>()
            .ok_or_else(|| failure("a String clone requires text"))?;
        Ok(value.to_string())
    }

    fn comparing() -> Option<&'static super::sequence::Comparing<String>> {
        Some(&COMPARING)
    }

    fn reading() -> Option<&'static super::reading::Reading<String>> {
        static READING: std::sync::OnceLock<super::reading::Reading<String>> =
            std::sync::OnceLock::new();
        Some(READING.get_or_init(|| {
            super::reading::Reading::from_traits(
                Some(super::reading::debugging::<String>()),
                Some(super::reading::displaying::<String>()),
                Some(super::reading::partial::<String>()),
                Some(super::reading::total::<String>()),
            )
        }))
    }

    fn invoke(
        mut value: Access<'_, String>,
        witness: &Witness,
        operation: Operation,
        arguments: &mut [Dynamic],
        _rest: Option<&mut crate::scope::Rest<'_>>,
    ) -> RawrResult {
        match operation {
            Operation::Equal { .. } => read(argument(arguments, 0)?, |rhs| {
                Ok((value.read().as_str() == rhs).into())
            }),
            Operation::Compare { reversed, .. } => read(argument(arguments, 0)?, |rhs| {
                let ordering = if reversed {
                    rhs.cmp(value.read())
                } else {
                    value.read().as_str().cmp(rhs)
                };
                Ok(crate::option::some((ordering as crate::INT).into()))
            }),
            Operation::Len => Ok(Dynamic::from_int(value.read().len() as crate::INT)),
            Operation::Capacity => Ok(Dynamic::from_int(value.read().capacity() as crate::INT)),
            Operation::Clone => Self::into_dynamic(value.read().clone(), witness),
            Operation::Store | Operation::Replace => {
                let replacement = Self::from_dynamic(argument(arguments, 0)?, witness)?;
                let value = value.write()?;
                if operation == Operation::Replace {
                    return Self::into_dynamic(std::mem::replace(value, replacement), witness);
                }
                *value = replacement;
                Ok(Dynamic::UNIT)
            }
            Operation::Take => Self::into_dynamic(std::mem::take(value.write()?), witness),
            Operation::Push => {
                let letter = argument(arguments, 0)?
                    .as_char()
                    .map_err(|_| failure("String::push requires a char"))?;
                value.write()?.push(letter);
                Ok(Dynamic::UNIT)
            }
            Operation::PushStr => read(argument(arguments, 0)?, |text| {
                value.write()?.push_str(text);
                Ok(Dynamic::UNIT)
            }),
            Operation::Insert => {
                let at = index(arguments, 0)?;
                let letter = argument(arguments, 1)?
                    .as_char()
                    .map_err(|_| failure("String::insert requires a char"))?;
                let value = value.write()?;
                boundary(value, at, true)?;
                value.insert(at, letter);
                Ok(Dynamic::UNIT)
            }
            Operation::InsertStr => {
                let at = index(arguments, 0)?;
                read(argument(arguments, 1)?, |text| {
                    let value = value.write()?;
                    boundary(value, at, true)?;
                    value.insert_str(at, text);
                    Ok(Dynamic::UNIT)
                })
            }
            Operation::Remove => {
                let at = index(arguments, 0)?;
                let value = value.write()?;
                boundary(value, at, false)?;
                Ok(Dynamic::from_char(value.remove(at)))
            }
            Operation::SplitOff => {
                let at = index(arguments, 0)?;
                let value = value.write()?;
                boundary(value, at, true)?;
                Self::into_dynamic(value.split_off(at), witness)
            }
            Operation::Clear => {
                value.write()?.clear();
                Ok(Dynamic::UNIT)
            }
            Operation::Truncate => {
                let at = index(arguments, 0)?;
                let value = value.write()?;
                if at < value.len() && !value.is_char_boundary(at) {
                    return Err(failure("String truncation is not on a character boundary"));
                }
                value.truncate(at);
                Ok(Dynamic::UNIT)
            }
            Operation::Pop => Ok(match value.write()?.pop() {
                Some(letter) => crate::option::some(Dynamic::from_char(letter)),
                None => crate::option::none(),
            }),
            Operation::Retain => {
                let value = value.write()?;
                retain_in_context(argument(arguments, 0)?, |predicate| value.retain(predicate))
            }
            Operation::Copy => Err(failure("a String cannot be copied")),
            _ => super::reading::invoke::<Self>(value, witness, operation, arguments),
        }
    }
}
