//! Module defining functions for evaluating a statement.

use super::{Caches, EvalContext, GlobalRuntimeState, Target};
use crate::ast::{
    ASTFlags, BinaryExpr, Expr, FlowControl, NominalEnumPattern, NominalLiteral, NominalPattern,
    NominalPayloadType, OpAssignment, Stmt, SwitchCasesCollection,
};
use crate::func::{get_builtin_op_assignment_fn, get_hasher};
use crate::tokenizer::Token;
use crate::types::dynamic::{AccessMode, Union};
use crate::{
    Dynamic, Engine, ImmutableString, RawrResult, RawrResultOf, Scope, VarDefInfo, ERR, INT,
};
use std::hash::{Hash, Hasher};

fn matches_nominal_payload_type(value: &Dynamic, expected: &NominalPayloadType) -> bool {
    match expected {
        NominalPayloadType::Unit => value.is_unit(),
        NominalPayloadType::Any => true,
        NominalPayloadType::Host(type_id) => value.type_id() == *type_id,
        NominalPayloadType::Script { identity, schema } => match &value.0 {
            Union::Struct(value, ..) => value.identity() == identity && value.schema() == schema,
            Union::Enum(value, ..) => value.identity() == identity && value.schema() == schema,
            _ => false,
        },
    }
}

/// What a pattern binds as it is matched: each binding's value, and the
/// path from the place matched to the value being matched now, which a
/// `&mut` binding of a place takes (`NominalPattern::Part`).
struct Collected<'a> {
    bindings: &'a mut Vec<(ImmutableString, Dynamic)>,
    path: crate::Array,
}

impl Collected<'_> {
    /// Match the part at `index` of the value being matched.
    fn part(
        &mut self,
        index: usize,
        value: &Dynamic,
        pattern: &NominalPattern,
        shape: &dyn Fn() -> String,
    ) -> Result<bool, String> {
        self.path.push(Dynamic::from_int(index as INT));
        let matched = collect_nominal(value, pattern, false, self, shape);
        self.path.pop();
        matched
    }
}

fn collect_nominal_pattern_bindings(
    value: &Dynamic,
    pattern: &NominalEnumPattern,
    root: bool,
    collected: &mut Collected,
) -> Result<bool, String> {
    let Union::Enum(nominal, ..) = &value.0 else {
        return if root {
            Ok(false)
        } else {
            Err(format!(
                "nominal enum '{}::{}' payload no longer matches its proven shape",
                pattern.enum_name, pattern.variant
            ))
        };
    };

    if nominal.identity() != &pattern.identity {
        return if root {
            Ok(false)
        } else {
            Err(format!(
                "nominal enum '{}::{}' payload no longer matches its proven shape",
                pattern.enum_name, pattern.variant
            ))
        };
    }
    if nominal.schema() != &pattern.schema {
        return Err(format!(
            "nominal enum '{}' has a stale schema while matching '{}::{}'",
            pattern.enum_name, pattern.enum_name, pattern.variant
        ));
    }
    if !pattern
        .variants
        .iter()
        .any(|variant| variant == nominal.variant().name())
    {
        return Err(format!(
            "nominal enum '{}' contains unknown variant '{}'",
            pattern.enum_name,
            nominal.variant().name()
        ));
    }
    if nominal.variant() != &pattern.variant {
        return Ok(false);
    }
    if nominal.payload().len() != pattern.payload.len() {
        return Err(format!(
            "nominal enum '{}::{}' contains {} payload value(s), expected {}",
            pattern.enum_name,
            pattern.variant,
            nominal.payload().len(),
            pattern.payload.len()
        ));
    }

    let original_bindings = collected.bindings.len();
    let shape = || {
        format!(
            "nominal enum '{}::{}' payload no longer matches its proven shape",
            pattern.enum_name, pattern.variant
        )
    };
    for (index, (value, child)) in nominal
        .payload()
        .iter()
        .zip(pattern.payload.iter())
        .enumerate()
    {
        if !collected.part(index, value, child, &shape)? {
            collected.bindings.truncate(original_bindings);
            return Ok(false);
        }
    }

    Ok(true)
}

/// Match `value` against `pattern`. A value of the wrong shape is a stale
/// proof below the root and a plain mismatch at the root.
fn collect_nominal(
    value: &Dynamic,
    pattern: &NominalPattern,
    root: bool,
    collected: &mut Collected,
    shape: &dyn Fn() -> String,
) -> Result<bool, String> {
    let mismatch = || if root { Ok(false) } else { Err(shape()) };
    match pattern {
        NominalPattern::Binding { name, expected } => {
            if !matches_nominal_payload_type(value, expected) {
                return Err(shape());
            }
            collected.bindings.push((name.clone(), value.clone()));
            Ok(true)
        }
        // The path to the part, which the binding reaches it by.
        NominalPattern::Part { name, expected } => {
            if !matches_nominal_payload_type(value, expected) {
                return Err(shape());
            }
            let path = Dynamic::from_array(collected.path.clone());
            collected.bindings.push((name.clone(), path));
            Ok(true)
        }
        NominalPattern::Discard { expected } => {
            if !matches_nominal_payload_type(value, expected) {
                return Err(shape());
            }
            Ok(true)
        }
        NominalPattern::Enum(child) => {
            collect_nominal_pattern_bindings(value, child, root, collected)
        }
        NominalPattern::Option(some) => {
            let shape = || "option payload no longer matches its proven shape".to_string();
            let Some(option) = value.read_lock::<crate::option::Value>() else {
                return if root { Ok(false) } else { Err(shape()) };
            };
            match (&*option, some) {
                (None, None) => Ok(true),
                (Some(_), None) | (None, Some(_)) => Ok(false),
                (Some(inner), Some(child)) => collected.part(0, inner, child, &shape),
            }
        }
        NominalPattern::Result(variant) => {
            let shape = || "result payload no longer matches its proven shape".to_string();
            let Some(result) = value.read_lock::<crate::result::Value>() else {
                return if root { Ok(false) } else { Err(shape()) };
            };
            match (&*result, variant) {
                (Ok(inner), Ok(child)) | (Err(inner), Err(child)) => {
                    collected.part(0, inner, child, &shape)
                }
                (Ok(_), Err(_)) | (Err(_), Ok(_)) => Ok(false),
            }
        }
        NominalPattern::Literal(literal) => match compare_literal(value, literal) {
            Some(ordering) => Ok(ordering == std::cmp::Ordering::Equal),
            None => mismatch(),
        },
        NominalPattern::Range {
            low,
            high,
            inclusive,
        } => {
            if let Some(low) = low {
                match compare_literal(value, low) {
                    Some(std::cmp::Ordering::Less) => return Ok(false),
                    Some(_) => {}
                    None => return mismatch(),
                }
            }
            if let Some(high) = high {
                match compare_literal(value, high) {
                    Some(std::cmp::Ordering::Greater) => return Ok(false),
                    Some(std::cmp::Ordering::Equal) if !inclusive => return Ok(false),
                    Some(_) => {}
                    None => return mismatch(),
                }
            }
            Ok(true)
        }
        NominalPattern::Tuple(members) => {
            let Ok(array) = value.as_array_ref() else {
                return mismatch();
            };
            if array.len() != members.len() {
                return mismatch();
            }
            let original = collected.bindings.len();
            for (index, (value, member)) in array.iter().zip(members.iter()).enumerate() {
                if !collected.part(index, value, member, shape)? {
                    collected.bindings.truncate(original);
                    return Ok(false);
                }
            }
            Ok(true)
        }
        NominalPattern::Struct(members) => {
            let Some(value) = value.read_lock::<crate::types::nominal::Struct>() else {
                return mismatch();
            };
            if value.fields().len() != members.len() {
                return mismatch();
            }
            let original = collected.bindings.len();
            for (index, (value, member)) in value.fields().iter().zip(members.iter()).enumerate() {
                if !collected.part(index, value, member, shape)? {
                    collected.bindings.truncate(original);
                    return Ok(false);
                }
            }
            Ok(true)
        }
        NominalPattern::Slice {
            before,
            rest,
            after,
        } => {
            let Ok(array) = value.as_array_ref() else {
                return mismatch();
            };
            collect_slice(
                &array,
                0,
                (before, rest.as_deref(), after),
                collected,
                shape,
            )
        }
        NominalPattern::Or(alternatives) => {
            let original = collected.bindings.len();
            for alternative in alternatives.iter() {
                if collect_nominal(value, alternative, root, collected, shape)? {
                    return Ok(true);
                }
                collected.bindings.truncate(original);
            }
            Ok(false)
        }
        NominalPattern::At {
            name,
            pattern,
            part,
        } => {
            let original = collected.bindings.len();
            let bound = if *part {
                Dynamic::from_array(collected.path.clone())
            } else {
                value.clone()
            };
            collected.bindings.push((name.clone(), bound));
            let matched = collect_nominal(value, pattern, root, collected, shape)?;
            if !matched {
                collected.bindings.truncate(original);
            }
            Ok(matched)
        }
    }
}

/// `value` against a pattern literal of its type; `None` for another type.
fn compare_literal(value: &Dynamic, literal: &NominalLiteral) -> Option<std::cmp::Ordering> {
    match literal {
        NominalLiteral::Int(expected) => value.as_int().ok().map(|actual| actual.cmp(expected)),
        NominalLiteral::Bool(expected) => value.as_bool().ok().map(|actual| actual.cmp(expected)),
        NominalLiteral::Char(expected) => value.as_char().ok().map(|actual| actual.cmp(expected)),
        NominalLiteral::Str(expected) => value
            .as_immutable_string_ref()
            .ok()
            .map(|actual| actual.as_str().cmp(expected.as_str())),
        NominalLiteral::Float(expected) => value
            .as_float()
            .ok()
            .and_then(|actual| actual.partial_cmp(&**expected)),
    }
}

/// Match `array`, the elements from `offset` of the array the path reaches,
/// against a slice pattern's elements before its rest, its rest and the
/// elements after it.
fn collect_slice(
    array: &[Dynamic],
    offset: usize,
    (before, rest, after): (
        &[NominalPattern],
        Option<&NominalPattern>,
        &[NominalPattern],
    ),
    collected: &mut Collected,
    shape: &dyn Fn() -> String,
) -> Result<bool, String> {
    let fixed = before.len() + after.len();
    if array.len() < fixed || rest.is_none() && array.len() != fixed {
        return Ok(false);
    }
    let middle = before.len()..array.len() - after.len();
    let original = collected.bindings.len();
    let ends = before
        .iter()
        .zip(array[..middle.start].iter().enumerate())
        .chain(
            after.iter().zip(
                array[middle.end..]
                    .iter()
                    .enumerate()
                    .map(|(index, value)| (middle.end + index, value)),
            ),
        );
    for (member, (index, value)) in ends {
        if !collected.part(offset + index, value, member, shape)? {
            collected.bindings.truncate(original);
            return Ok(false);
        }
    }
    // A rest that binds takes a copy of the middle, which no path reaches:
    // a `&mut` binding in it is refused where it is lowered.
    if let Some(rest) = rest.filter(|rest| !matches!(rest, NominalPattern::Discard { .. })) {
        let middle = Dynamic::from_array(array[middle].to_vec());
        collected.path.push(Dynamic::UNIT);
        let matched = collect_nominal(&middle, rest, false, collected, shape);
        collected.path.pop();
        if !matched? {
            collected.bindings.truncate(original);
            return Ok(false);
        }
    }
    Ok(true)
}

/// Match a range of an array's elements (`&mut v[1..3]`), `array` from
/// `offset`, against one arm's root pattern.
fn collect_window(
    array: &[Dynamic],
    offset: usize,
    pattern: &NominalPattern,
    collected: &mut Collected,
) -> Result<bool, String> {
    let shape = || "a slice no longer matches its proven shape".to_string();
    match pattern {
        NominalPattern::Slice {
            before,
            rest,
            after,
        } => collect_slice(
            array,
            offset,
            (before, rest.as_deref(), after),
            collected,
            &shape,
        ),
        NominalPattern::Discard { .. } => Ok(true),
        NominalPattern::Or(alternatives) => {
            let original = collected.bindings.len();
            for alternative in alternatives.iter() {
                if collect_window(array, offset, alternative, collected)? {
                    return Ok(true);
                }
                collected.bindings.truncate(original);
            }
            Ok(false)
        }
        _ => Err(shape()),
    }
}

/// Match the scrutinee against one arm's root pattern. A `&mut` binding
/// of a place takes the path below `path`, the place's own.
fn collect_nominal_root(
    value: &Dynamic,
    pattern: &NominalPattern,
    bindings: &mut Vec<(ImmutableString, Dynamic)>,
    path: crate::Array,
) -> Result<bool, String> {
    let shape = || "a match subject no longer matches its proven shape".to_string();
    collect_nominal(
        value,
        pattern,
        true,
        &mut Collected { bindings, path },
        &shape,
    )
}

/// Match `Some(&mut place)` against one arm's root pattern, where `value`
/// is the place, at `path`: `Some(pattern)` matches the pattern on the
/// place itself.
fn collect_some_place(
    value: &Dynamic,
    pattern: &NominalPattern,
    bindings: &mut Vec<(ImmutableString, Dynamic)>,
    path: &crate::Array,
) -> Result<bool, String> {
    match pattern {
        NominalPattern::Option(Some(child)) => {
            collect_nominal_root(value, child, bindings, path.clone())
        }
        NominalPattern::Option(None) => Ok(false),
        NominalPattern::Discard { .. } => Ok(true),
        NominalPattern::Or(alternatives) => {
            for alternative in alternatives.iter() {
                if collect_some_place(value, alternative, bindings, path)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        _ => Err("a binding of `Some(&mut place)` itself is not supported in Rawr yet".into()),
    }
}

impl Engine {
    /// If the value is a string, intern it.
    #[inline(always)]
    fn intern_string(&self, value: Dynamic) -> Dynamic {
        match value.0 {
            Union::Str(s, ..) => self.get_interned_string(s).into(),
            _ => value,
        }
    }

    /// Match one arm's root pattern on the place the local `holder` holds
    /// the path of, where it is. With `some`, the subject is `Some(&mut
    /// place)`, or `None` when `holder` holds `()`.
    #[allow(clippy::too_many_arguments)]
    fn collect_place(
        scope: &mut Scope,
        this_ptr: Option<&mut Dynamic>,
        holder: &str,
        some: bool,
        pattern: &NominalPattern,
        bindings: &mut Vec<(ImmutableString, Dynamic)>,
        expr: &Expr,
    ) -> RawrResultOf<Result<bool, String>> {
        let held = scope
            .search(holder)
            // A path a closure captured is shared: its indexes are read out.
            .map(|index| scope.get_mut_by_index(index).flatten_clone())
            .ok_or_else(|| ERR::ErrorVariableNotFound(holder.to_string(), expr.position()))?;
        if some && held.is_unit() {
            let none = crate::option::none();
            return Ok(collect_nominal_root(
                &none,
                pattern,
                bindings,
                crate::Array::new(),
            ));
        }
        let missing = || ERR::ErrorVariableNotFound(holder.to_string(), expr.position());
        let mut path = if held.is_array() {
            held.into_array().map_err(|_| missing())?
        } else {
            vec![held]
        };
        // A range of an array's elements ends its path (`&mut v[1..3]`).
        let window = path.last().and_then(|last| {
            let range = last
                .read_lock::<crate::ExclusiveRange>()
                .map(|range| (range.start, range.end, false));
            range.or_else(|| {
                last.read_lock::<crate::InclusiveRange>()
                    .map(|range| (*range.start(), *range.end(), true))
            })
        });
        if window.is_some() {
            path.pop();
        }
        let (root, steps) = Self::path_parts(&path).ok_or_else(missing)?;
        let target = Self::place_at(scope, this_ptr, &root, &steps, holder, expr)?;
        let place = target.as_ref();
        let Some((low, high, inclusive)) = window else {
            let place = place.read_lock::<Dynamic>().ok_or_else(missing)?;
            return Ok(if some {
                collect_some_place(&place, pattern, bindings, &path)
            } else {
                collect_nominal_root(&place, pattern, bindings, path)
            });
        };
        let array = place.as_array_ref().map_err(|_| missing())?;
        let len = array.len();
        let low = usize::try_from(low).unwrap_or(usize::MAX);
        let high = match high {
            -1 => len,
            high => usize::try_from(high).unwrap_or(usize::MAX),
        };
        let high = if inclusive {
            high.saturating_add(1)
        } else {
            high
        };
        let fail = |message: String| Err(ERR::ErrorRuntime(message.into(), expr.position()).into());
        if high > len {
            return fail(format!(
                "range end index {high} out of range for slice of length {len}"
            ));
        }
        if low > high {
            return fail(format!("slice index starts at {low} but ends at {high}"));
        }
        let collected = &mut Collected { bindings, path };
        Ok(collect_window(&array[low..high], low, pattern, collected))
    }

    /// Evaluate a statements block.
    pub(crate) fn eval_stmt_block(
        &self,
        global: &mut GlobalRuntimeState,
        caches: &mut Caches,
        scope: &mut Scope,
        this_ptr: Option<&mut Dynamic>,
        statements: &[Stmt],
        restore_orig_state: bool,
    ) -> RawrResult {
        if statements.is_empty() {
            return Ok(Dynamic::UNIT);
        }

        // Restore scope at end of block if necessary
        defer! { scope if restore_orig_state => rewind; let orig_scope_len = scope.len(); }

        // Restore global state at end of block if necessary
        let orig_always_search_scope = global.always_search_scope;
        let orig_imports_len = global.num_imports();

        if restore_orig_state {
            global.scope_level += 1;
        }

        defer! { global if restore_orig_state => move |g| {
            g.scope_level -= 1;

            g.truncate_imports(orig_imports_len);

            // The impact of new local variables goes away at the end of a block
            // because any new variables introduced will go out of scope
            g.always_search_scope = orig_always_search_scope;
        }}

        // Pop new function resolution caches at end of block
        defer! {
            caches => rewind_fn_resolution_caches;
            let orig_fn_resolution_caches_len = caches.fn_resolution_caches_len();
        }

        // Run the statements
        self.eval_stmts_from(
            global,
            caches,
            scope,
            this_ptr,
            statements,
            restore_orig_state,
            orig_fn_resolution_caches_len,
        )
    }

    /// Evaluate a block's statements from some point on, inside the block's
    /// state. A `let` that binds a scope frame a call made (D558 tier 1, a
    /// [`crate::scope::Opening`]) opens it, and the statements after it
    /// run inside the frame, as the rest of the Rust scope the value is
    /// bound in. A statement whose value is dropped opens its frame with
    /// nothing after.
    #[allow(clippy::too_many_arguments)]
    fn eval_stmts_from(
        &self,
        global: &mut GlobalRuntimeState,
        caches: &mut Caches,
        scope: &mut Scope,
        mut this_ptr: Option<&mut Dynamic>,
        statements: &[Stmt],
        restore_orig_state: bool,
        orig_fn_resolution_caches_len: usize,
    ) -> RawrResult {
        let mut result = Dynamic::UNIT;
        for (k, stmt) in statements.iter().enumerate() {
            let orig_imports_len = global.num_imports();

            result = self.eval_stmt(
                global,
                caches,
                scope,
                this_ptr.as_deref_mut(),
                stmt,
                restore_orig_state,
            )?;

            // `let p = Parser::new(&text);`: the rest of the block runs in
            // the frame the call made, with `p` bound to its value.
            if let Stmt::Var(x, ..) = stmt {
                let at = match x.2 {
                    Some(index) => scope.len() - index.get(),
                    None => scope.len() - 1,
                };
                if crate::scope::is_opening(scope.get_mut_by_index(at)) {
                    let bound = std::mem::take(scope.get_mut_by_index(at));
                    let access = bound.access_mode();
                    let opening = bound.cast::<crate::scope::Opening>();
                    let rest = &statements[k + 1..];
                    let mut continuation = |mut value: Dynamic| -> RawrResult {
                        value.set_access_mode(access);
                        *scope.get_mut_by_index(at) = value;
                        self.eval_stmts_from(
                            global,
                            caches,
                            scope,
                            this_ptr.as_deref_mut(),
                            rest,
                            restore_orig_state,
                            orig_fn_resolution_caches_len,
                        )
                    };
                    return opening.open(&mut continuation);
                }
            } else if k + 1 < statements.len() {
                result = crate::scope::settle(result)?;
            }

            if matches!(stmt, Stmt::Import(..)) {
                // Get the extra modules - see if any functions are marked global.
                // Without global functions, the extra modules never affect function resolution.
                if global
                    .scan_imports_raw()
                    .skip(orig_imports_len)
                    .any(|(.., m)| m.contains_indexed_global_functions())
                {
                    // Different scenarios where the cache must be cleared - notice that this is
                    // expensive as all function resolutions must start again
                    if caches.fn_resolution_caches_len() > orig_fn_resolution_caches_len {
                        // When new module is imported with global functions and there is already
                        // a new cache, just clear it
                        caches.fn_resolution_cache_mut().clear();
                    } else if restore_orig_state {
                        // When new module is imported with global functions, push a new cache
                        caches.push_fn_resolution_cache();
                    } else {
                        // When the block is to be evaluated in-place, just clear the current cache
                        caches.fn_resolution_cache_mut().clear();
                    }
                }
            }
        }
        Ok(result)
    }

    /// Evaluate an op-assignment statement.
    pub(crate) fn eval_op_assignment(
        &self,
        global: &mut GlobalRuntimeState,
        caches: &mut Caches,
        op_info: &OpAssignment,
        root: &Expr,
        target: &mut Target,
        mut new_val: Dynamic,
    ) -> RawrResultOf<()> {
        // Assignment to constant variable?
        if target.as_ref().is_read_only() {
            let name = root.get_variable_name(false).unwrap_or_default();
            let pos = root.start_position();
            return Err(ERR::ErrorAssignmentToConstant(name.to_string(), pos).into());
        }

        let pos = op_info.position();

        if let Some((hash_x, hash, op_x, op_x_str, op, op_str)) = op_info.get_op_assignment_info() {
            let mut lock_guard = target.as_mut().write_lock::<Dynamic>().unwrap();
            let mut done = false;

            // Short-circuit built-in op-assignments if under Fast Operators mode
            if self.fast_operators() {
                #[allow(clippy::wildcard_imports)]
                use Token::*;

                done = true;

                // For extremely simple primary data operations, do it directly
                // to avoid the overhead of calling a function.
                match (&mut lock_guard.0, &mut new_val.0) {
                    (Union::Bool(b1, ..), Union::Bool(b2, ..)) => match op_x {
                        AndAssign => *b1 = *b1 && *b2,
                        OrAssign => *b1 = *b1 || *b2,
                        XOrAssign => *b1 ^= *b2,
                        _ => done = false,
                    },
                    (Union::Int(n1, ..), Union::Int(n2, ..)) => {
                        #[allow(clippy::wildcard_imports)]
                        use crate::packages::arithmetic::arith_basic::INT::functions::*;

                        match op_x {
                            PlusAssign => {
                                *n1 = add(*n1, *n2).map_err(|err| err.fill_position(pos))?
                            }
                            MinusAssign => {
                                *n1 = subtract(*n1, *n2).map_err(|err| err.fill_position(pos))?
                            }
                            MultiplyAssign => {
                                *n1 = multiply(*n1, *n2).map_err(|err| err.fill_position(pos))?
                            }
                            DivideAssign => {
                                *n1 = divide(*n1, *n2).map_err(|err| err.fill_position(pos))?
                            }
                            ModuloAssign => {
                                *n1 = modulo(*n1, *n2).map_err(|err| err.fill_position(pos))?
                            }
                            _ => done = false,
                        }
                    }
                    (Union::Float(f1, ..), Union::Float(f2, ..)) => match op_x {
                        PlusAssign => **f1 += **f2,
                        MinusAssign => **f1 -= **f2,
                        MultiplyAssign => **f1 *= **f2,
                        DivideAssign => **f1 /= **f2,
                        ModuloAssign => **f1 %= **f2,
                        _ => done = false,
                    },
                    (Union::Float(f1, ..), Union::Int(n2, ..)) => match op_x {
                        PlusAssign => **f1 += *n2 as crate::FLOAT,
                        MinusAssign => **f1 -= *n2 as crate::FLOAT,
                        MultiplyAssign => **f1 *= *n2 as crate::FLOAT,
                        DivideAssign => **f1 /= *n2 as crate::FLOAT,
                        ModuloAssign => **f1 %= *n2 as crate::FLOAT,
                        _ => done = false,
                    },
                    _ => done = false,
                }

                if !done
                    && let Some((func, need_context)) =
                        get_builtin_op_assignment_fn(op_x, &lock_guard, &new_val)
                {
                    // We may not need to bump the level because built-in's do not need it.
                    //defer! { let orig_level = global.level; global.level += 1 }

                    let args = &mut [&mut *lock_guard, &mut new_val];
                    let context = need_context
                        .then(|| (self, op_x_str, global.source(), &*global, pos).into());
                    let _ = func(context, args).map_err(|err| err.fill_position(pos))?;
                    done = true;
                }
            }

            if !done {
                let opx = Some(op_x);
                let args = &mut [&mut *lock_guard, &mut new_val];

                match self.exec_native_fn_call(
                    global, caches, op_x_str, opx, hash_x, args, true, false, false, pos,
                ) {
                    Ok(_) => (),
                    Err(err) if matches!(*err, ERR::ErrorFunctionNotFound(ref f, ..) if f.starts_with(op_x_str)) =>
                    {
                        // Expand to `var = var op rhs`
                        let op = Some(op);

                        *args[0] = self
                            .exec_native_fn_call(
                                global, caches, op_str, op, hash, args, true, false, false, pos,
                            )?
                            .0;
                    }
                    Err(err) => return Err(err),
                }

                self.check_data_size(&*args[0], root.position())?;
            }
        } else {
            // Normal assignment
            match target {
                // Lock it again just in case it is shared
                Target::RefMut(_) | Target::TempValue(_) => {
                    *target.as_mut().write_lock::<Dynamic>().unwrap() = new_val
                }
                #[allow(unreachable_patterns)]
                _ => *target.as_mut() = new_val,
            }
        }

        target.propagate_changed_value(pos)
    }

    /// Evaluate a statement.
    pub(crate) fn eval_stmt(
        &self,
        global: &mut GlobalRuntimeState,
        caches: &mut Caches,
        scope: &mut Scope,
        mut this_ptr: Option<&mut Dynamic>,
        stmt: &Stmt,
        rewind_scope: bool,
    ) -> RawrResult {
        self.track_operation(global, stmt.position())?;

        #[cfg(feature = "debugging")]
        let reset = self.dbg_reset(global, caches, scope, this_ptr.as_deref_mut(), stmt)?;
        #[cfg(feature = "debugging")]
        defer! { global if Some(reset) => move |g| g.debugger_mut().reset_status(reset) }

        match stmt {
            // No-op
            Stmt::Noop(..) => Ok(Dynamic::UNIT),

            // Expression as statement
            Stmt::Expr(expr) => self
                .eval_expr(global, caches, scope, this_ptr, expr)
                .map(Dynamic::flatten),

            // Block scope
            Stmt::Block(stmts, ..) => {
                if stmts.is_empty() {
                    Ok(Dynamic::UNIT)
                } else {
                    self.eval_stmt_block(global, caches, scope, this_ptr, stmts.statements(), true)
                }
            }

            // Function call
            Stmt::FnCall(x, pos) => {
                self.eval_fn_call_expr(global, caches, scope, this_ptr, x, *pos)
            }

            // Assignment
            Stmt::Assignment(x, ..) => {
                let (op_info, BinaryExpr { lhs, rhs }) = &**x;

                if let Expr::ThisPtr(..) = lhs {
                    if this_ptr.is_none() {
                        return Err(ERR::ErrorUnboundThis(lhs.position()).into());
                    }

                    {
                        use std::convert::TryInto;

                        let rhs_val = self
                            .eval_expr(global, caches, scope, this_ptr.as_deref_mut(), rhs)?
                            .flatten();

                        self.track_operation(global, lhs.position())?;

                        let target = &mut this_ptr.unwrap().try_into()?;

                        self.eval_op_assignment(global, caches, op_info, lhs, target, rhs_val)?;
                    }
                } else if let Expr::Variable(x, ..) = lhs {
                    let rhs_val = self
                        .eval_expr(global, caches, scope, this_ptr.as_deref_mut(), rhs)?
                        .flatten();

                    self.track_operation(global, lhs.position())?;

                    let mut target = self.search_namespace(global, caches, scope, this_ptr, lhs)?;

                    let is_temp_result = !target.is_ref();

                    // Also handle case where target is a `Dynamic` shared value
                    // (returned by a variable resolver, for example)
                    let is_temp_result = is_temp_result && !target.is_shared();

                    // Cannot assign to temp result from expression
                    if is_temp_result {
                        return Err(ERR::ErrorAssignmentToConstant(
                            x.1.to_string(),
                            lhs.position(),
                        )
                        .into());
                    }

                    self.eval_op_assignment(global, caches, op_info, lhs, &mut target, rhs_val)?;
                } else {
                    {
                        let rhs_val = self
                            .eval_expr(global, caches, scope, this_ptr.as_deref_mut(), rhs)?
                            .flatten();
                        let _new_val = Some((self.intern_string(rhs_val), op_info));

                        // Must be either `var[index] op= val` or `var.prop op= val`.
                        // The return value of any op-assignment (should be `()`) is thrown away and not used.
                        let _ = match lhs {
                            // this op= rhs (handled above)
                            Expr::ThisPtr(..) => {
                                unreachable!("Expr::ThisPtr case is already handled")
                            }
                            // name op= rhs (handled above)
                            Expr::Variable(..) => {
                                unreachable!("Expr::Variable case is already handled")
                            }
                            // idx_lhs[idx_expr] op= rhs
                            Expr::Index(..) => self.eval_dot_index_chain(
                                global, caches, scope, this_ptr, lhs, _new_val,
                            ),
                            // dot_lhs.dot_rhs op= rhs
                            Expr::Dot(..) => self.eval_dot_index_chain(
                                global, caches, scope, this_ptr, lhs, _new_val,
                            ),
                            _ => unreachable!("cannot assign to expression: {:?}", lhs),
                        }?;
                    }
                }

                Ok(Dynamic::UNIT)
            }

            // Variable definition
            Stmt::Var(x, options, pos) => {
                if !self.allow_shadowing() && scope.contains(x.0.as_str()) {
                    return Err(ERR::ErrorVariableExists(x.0.as_str().to_string(), *pos).into());
                }

                // Let/const statement
                let (var_name, expr, index) = &**x;

                let access = if options.intersects(ASTFlags::CONSTANT) {
                    AccessMode::ReadOnly
                } else {
                    AccessMode::ReadWrite
                };
                let export = options.intersects(ASTFlags::EXPORTED);

                // Check variable definition filter
                if let Some(ref filter) = self.def_var_filter {
                    let will_shadow = scope.contains(var_name.as_str());
                    let is_const = access == AccessMode::ReadOnly;
                    let info = VarDefInfo::new(
                        var_name.as_str(),
                        is_const,
                        global.scope_level,
                        will_shadow,
                    );
                    let orig_scope_len = scope.len();
                    let context =
                        EvalContext::new(self, global, caches, scope, this_ptr.as_deref_mut());
                    let filter_result = filter(true, info, context);

                    if orig_scope_len != scope.len() {
                        // The scope is changed, always search from now on
                        global.always_search_scope = true;
                    }

                    if !filter_result? {
                        return Err(ERR::ErrorForbiddenVariable(
                            var_name.as_str().to_string(),
                            *pos,
                        )
                        .into());
                    }
                }

                // Guard against too many variables
                if index.is_none() && scope.len() >= self.max_variables() {
                    return Err(ERR::ErrorTooManyVariables(*pos).into());
                }

                // Evaluate initial value. A place's shared handle
                // (`__rawr_share`) stays the handle: flattening it would
                // copy the value it shares.
                let value = self.eval_expr(global, caches, scope, this_ptr, expr)?;
                let value = match expr {
                    Expr::FnCall(call, ..) if call.name == crate::engine::KEYWORD_SHARE => value,
                    _ => value.flatten(),
                };
                let mut value = self.intern_string(value);

                let _alias = if !rewind_scope {
                    // Put global constants into global module
                    if global.scope_level == 0
                        && access == AccessMode::ReadOnly
                        && global.lib.iter().any(|m| !m.is_empty())
                    {
                        crate::func::locked_write(global.constants.get_or_insert_with(|| {
                            crate::Shared::new(
                                crate::Locked::new(std::collections::BTreeMap::new()),
                            )
                        }))
                        .unwrap()
                        .insert(var_name.name.clone(), value.clone());
                    }

                    export.then_some(var_name)
                } else if !export {
                    None
                } else {
                    unreachable!("exported variable not on global level");
                };

                match index {
                    Some(index) => {
                        value.set_access_mode(access);
                        *scope.get_mut_by_index(scope.len() - index.get()) = value;
                    }
                    _ => {
                        scope.push_entry(var_name.name.clone(), access, value);
                    }
                }

                if let Some(alias) = _alias {
                    scope.add_alias_by_index(scope.len() - 1, alias.as_str().into());
                }

                Ok(Dynamic::UNIT)
            }

            // If statement
            Stmt::If(x, ..) => {
                let FlowControl { expr, body, branch } = &**x;

                let guard_val = self
                    .eval_expr(global, caches, scope, this_ptr.as_deref_mut(), expr)?
                    .as_bool()
                    .map_err(|typ| self.make_type_mismatch_err::<bool>(typ, expr.position()))?;

                if guard_val && !body.is_empty() {
                    self.eval_stmt_block(global, caches, scope, this_ptr, body.statements(), true)
                } else if !guard_val && !branch.is_empty() {
                    self.eval_stmt_block(global, caches, scope, this_ptr, branch.statements(), true)
                } else {
                    Ok(Dynamic::UNIT)
                }
            }

            // Switch statement
            Stmt::Switch(x, ..) => {
                let (
                    expr,
                    SwitchCasesCollection {
                        expressions,
                        cases,
                        def_case,
                        enum_cases,
                        ranges,
                    },
                ) = &**x;

                let mut result = None;

                // A switch on a place (`__rawr_through_x`, or
                // `__rawr_some_through_x` for `Some(&mut place)`) matches it
                // where it is, and a `&mut` binding takes its part's path.
                let place = match expr {
                    Expr::Variable(v, None, ..) if v.0.is_none() => {
                        v.1.strip_prefix(crate::engine::KEYWORD_THROUGH)
                            .map(|holder| (holder, false))
                            .or_else(|| {
                                v.1.strip_prefix(crate::engine::KEYWORD_SOME_THROUGH)
                                    .map(|holder| (holder, true))
                            })
                    }
                    _ => None,
                };
                let value = match place {
                    Some(_) => Dynamic::UNIT,
                    None => self.eval_expr(global, caches, scope, this_ptr.as_deref_mut(), expr)?,
                };

                if place.is_none() && value.is_hashable() {
                    let hasher = &mut get_hasher();
                    value.hash(hasher);
                    let hash = hasher.finish();

                    // First check hashes
                    if let Some(case_blocks_list) = cases.get(&hash) {
                        debug_assert!(!case_blocks_list.is_empty());

                        for &index in case_blocks_list {
                            let block = &expressions[index];

                            let cond_result = match block.lhs {
                                Expr::BoolConstant(b, ..) => b,
                                ref c => self
                                    .eval_expr(global, caches, scope, this_ptr.as_deref_mut(), c)?
                                    .as_bool()
                                    .map_err(|typ| {
                                        self.make_type_mismatch_err::<bool>(typ, c.position())
                                    })?,
                            };

                            if cond_result {
                                result = Some(&block.rhs);
                                break;
                            }
                        }
                    } else if !ranges.is_empty() {
                        // Then check integer ranges
                        for r in ranges.iter().filter(|r| r.contains(&value)) {
                            let BinaryExpr { lhs, rhs } = &expressions[r.index()];

                            let cond_result = match lhs {
                                Expr::BoolConstant(b, ..) => *b,
                                c => self
                                    .eval_expr(global, caches, scope, this_ptr.as_deref_mut(), c)?
                                    .as_bool()
                                    .map_err(|typ| {
                                        self.make_type_mismatch_err::<bool>(typ, c.position())
                                    })?,
                            };

                            if cond_result {
                                result = Some(rhs);
                                break;
                            }
                        }
                    }
                }

                if result.is_none() {
                    for case in enum_cases {
                        let bindings = if let Some(pattern) = &case.nominal {
                            let mut bindings = Vec::new();
                            let matched = match place {
                                None => collect_nominal_root(
                                    &value,
                                    pattern,
                                    &mut bindings,
                                    crate::Array::new(),
                                ),
                                Some((holder, some)) => Self::collect_place(
                                    scope,
                                    this_ptr.as_deref_mut(),
                                    holder,
                                    some,
                                    pattern,
                                    &mut bindings,
                                    expr,
                                )?,
                            };
                            match matched {
                                Ok(true) => {}
                                Ok(false) => continue,
                                Err(message) => {
                                    return Err(
                                        ERR::ErrorRuntime(message.into(), expr.position()).into()
                                    )
                                }
                            }
                            // Or-pattern alternatives bind the same names in
                            // their own order.
                            debug_assert_eq!(
                                {
                                    let mut names = case
                                        .bindings
                                        .iter()
                                        .flatten()
                                        .map(|name| name.as_str())
                                        .collect::<Vec<_>>();
                                    names.sort_unstable();
                                    names
                                },
                                {
                                    let mut names = bindings
                                        .iter()
                                        .map(|(name, _)| name.as_str())
                                        .collect::<Vec<_>>();
                                    names.sort_unstable();
                                    names
                                },
                            );
                            bindings
                        } else {
                            if value.type_id() != case.type_id {
                                continue;
                            }

                            let Some(registration) =
                                self.registered_enums.get(case.enum_name.as_str())
                            else {
                                let message = format!(
                                    "enum registration '{}' required by the compiled switch is not available",
                                    case.enum_name
                                );
                                return Err(
                                    ERR::ErrorRuntime(message.into(), expr.position()).into()
                                );
                            };

                            if registration.type_id() != case.type_id {
                                let message = format!(
                                    "enum registration '{}' required by the compiled switch has a different Rust type",
                                    case.enum_name
                                );
                                return Err(
                                    ERR::ErrorRuntime(message.into(), expr.position()).into()
                                );
                            }

                            let Some(compiled_variant) = registration.variant(case.variant_index)
                            else {
                                let message = format!(
                                    "enum registration '{}::{}' required by the compiled switch no longer contains variant slot {}",
                                    case.enum_name, case.variant_name, case.variant_index
                                );
                                return Err(
                                    ERR::ErrorRuntime(message.into(), expr.position()).into()
                                );
                            };
                            if compiled_variant.name != case.variant_name
                                || compiled_variant.arity != case.bindings.len()
                            {
                                let message = format!(
                                    "enum registration '{}::{}' required by the compiled switch changed shape at variant slot {}",
                                    case.enum_name, case.variant_name, case.variant_index
                                );
                                return Err(
                                    ERR::ErrorRuntime(message.into(), expr.position()).into()
                                );
                            }

                            let Some((variant_index, payload)) = registration.extract(&value)
                            else {
                                continue;
                            };

                            let Some(variant) = registration.variant(variant_index) else {
                                let message = format!(
                                    "enum extractor for '{}' returned unknown variant index {variant_index}",
                                    case.enum_name
                                );
                                return Err(
                                    ERR::ErrorRuntime(message.into(), expr.position()).into()
                                );
                            };

                            if payload.len() != variant.arity {
                                let message = format!(
                                    "enum extractor for '{}::{}' returned {} payload value(s), expected {}",
                                    case.enum_name,
                                    variant.name,
                                    payload.len(),
                                    variant.arity
                                );
                                return Err(
                                    ERR::ErrorRuntime(message.into(), expr.position()).into()
                                );
                            }

                            if variant_index != case.variant_index {
                                continue;
                            }

                            case.bindings
                                .iter()
                                .zip(payload)
                                .filter_map(|(binding, value)| {
                                    binding.as_ref().map(|name| (name.clone(), value))
                                })
                                .collect()
                        };

                        let matched = {
                            defer! { scope => rewind; let original_scope_len = scope.len(); }

                            for (name, value) in bindings {
                                if scope.len() >= self.max_variables() {
                                    return Err(ERR::ErrorTooManyVariables(expr.position()).into());
                                }

                                scope.push_entry(name, AccessMode::ReadWrite, value);
                            }

                            let block = &expressions[case.index];
                            let condition = match block.lhs {
                                Expr::BoolConstant(value, ..) => value,
                                ref guard => self
                                    .eval_expr(
                                        global,
                                        caches,
                                        scope,
                                        this_ptr.as_deref_mut(),
                                        guard,
                                    )?
                                    .as_bool()
                                    .map_err(|typ| {
                                        self.make_type_mismatch_err::<bool>(typ, guard.position())
                                    })?,
                            };

                            if condition {
                                Some(self.eval_expr(
                                    global,
                                    caches,
                                    scope,
                                    this_ptr.as_deref_mut(),
                                    &block.rhs,
                                ))
                            } else {
                                None
                            }
                        };

                        if let Some(result) = matched {
                            return result;
                        }
                    }
                }

                result
                    .or_else(|| def_case.as_ref().map(|&index| &expressions[index].rhs))
                    .map_or(Ok(Dynamic::UNIT), |expr| {
                        self.eval_expr(global, caches, scope, this_ptr, expr)
                    })
            }

            // Loop
            Stmt::While(x, ..)
                if matches!(x.expr, Expr::Unit(..) | Expr::BoolConstant(true, ..)) =>
            {
                let FlowControl { body, .. } = &**x;

                if body.is_empty() {
                    loop {
                        self.track_operation(global, body.position())?;
                    }
                }

                loop {
                    let this_ptr = this_ptr.as_deref_mut();
                    let statements = body.statements();

                    match self
                        .eval_stmt_block(global, caches, scope, this_ptr, statements, true)
                        .and_then(crate::scope::settle)
                    {
                        Ok(..) => (),
                        Err(err) => match *err {
                            ERR::LoopBreak(false, ..) => (),
                            ERR::LoopBreak(true, value, ..) => break Ok(value),
                            _ => break Err(err),
                        },
                    }
                }
            }

            // While loop
            Stmt::While(x, ..) => {
                let FlowControl { expr, body, .. } = &**x;

                loop {
                    let condition = self
                        .eval_expr(global, caches, scope, this_ptr.as_deref_mut(), expr)?
                        .as_bool()
                        .map_err(|typ| self.make_type_mismatch_err::<bool>(typ, expr.position()))?;

                    if !condition {
                        break Ok(Dynamic::UNIT);
                    }

                    if body.is_empty() {
                        continue;
                    }

                    let this_ptr = this_ptr.as_deref_mut();
                    let statements = body.statements();

                    match self
                        .eval_stmt_block(global, caches, scope, this_ptr, statements, true)
                        .and_then(crate::scope::settle)
                    {
                        Ok(..) => (),
                        Err(err) => match *err {
                            ERR::LoopBreak(false, ..) => (),
                            ERR::LoopBreak(true, value, ..) => break Ok(value),
                            _ => break Err(err),
                        },
                    }
                }
            }

            // Do loop
            Stmt::Do(x, options, ..) => {
                let FlowControl { expr, body, .. } = &**x;
                let is_while = !options.intersects(ASTFlags::NEGATED);

                loop {
                    if !body.is_empty() {
                        let this_ptr = this_ptr.as_deref_mut();
                        let statements = body.statements();

                        match self
                            .eval_stmt_block(global, caches, scope, this_ptr, statements, true)
                            .and_then(crate::scope::settle)
                        {
                            Ok(..) => (),
                            Err(err) => match *err {
                                ERR::LoopBreak(false, ..) => continue,
                                ERR::LoopBreak(true, value, ..) => break Ok(value),
                                _ => break Err(err),
                            },
                        }
                    }

                    let condition = self
                        .eval_expr(global, caches, scope, this_ptr.as_deref_mut(), expr)?
                        .as_bool()
                        .map_err(|typ| self.make_type_mismatch_err::<bool>(typ, expr.position()))?;

                    if condition ^ is_while {
                        break Ok(Dynamic::UNIT);
                    }
                }
            }

            // For loop
            Stmt::For(x, ..) => {
                let (var_name, counter, FlowControl { expr, body, .. }) = &**x;

                // Guard against too many variables
                if scope.len() >= self.max_variables() - usize::from(counter.is_some()) {
                    return Err(ERR::ErrorTooManyVariables(var_name.pos).into());
                }

                let iter_obj = self
                    .eval_expr(global, caches, scope, this_ptr.as_deref_mut(), expr)?
                    .flatten();

                let iter_type = iter_obj.type_id();

                // lib should only contain scripts, so technically they cannot have iterators

                // Search order:
                // 1) Global namespace - functions registered via Engine::register_XXX
                // 2) Global modules - packages
                // 3) Imported modules - functions marked with global namespace
                // 4) Global sub-modules - functions marked with global namespace
                let iter_func = self
                    .global_modules
                    .iter()
                    .find_map(|m| m.get_iter(iter_type));

                let iter_func = iter_func
                    .or_else(|| global.get_iter(iter_type))
                    .or_else(|| {
                        self.global_sub_modules
                            .values()
                            .find_map(|m| m.get_qualified_iter(iter_type))
                    });

                let iter_func = iter_func.ok_or_else(|| ERR::ErrorFor(expr.start_position()))?;

                // Restore scope at end of statement
                defer! { scope => rewind; let orig_scope_len = scope.len(); }

                // Add the loop variables
                let counter_index = counter.as_ref().map(|counter| {
                    scope.push(counter.name.clone(), 0 as INT);
                    scope.len() - 1
                });

                scope.push(var_name.name.clone(), ());
                let index = scope.len() - 1;

                let mut result = Dynamic::UNIT;

                if body.is_empty() {
                    for _ in iter_func(iter_obj) {
                        self.track_operation(global, body.position())?;
                    }
                } else {
                    let mut index_value: INT = -1;

                    for iter_value in iter_func(iter_obj) {
                        {
                            index_value = index_value.checked_add(1).ok_or_else(|| {
                                ERR::ErrorArithmetic(
                                    format!("for-loop counter overflow: {index_value}"),
                                    var_name.pos,
                                )
                            })?;
                        }

                        // Increment counter
                        if let Some(counter_index) = counter_index {
                            *scope.get_mut_by_index(counter_index).write_lock().unwrap() =
                                Dynamic::from_int(index_value);
                        }

                        // Set loop value
                        let value = iter_value
                            .map_err(|err| err.fill_position(expr.position()))?
                            .flatten();

                        *scope.get_mut_by_index(index).write_lock().unwrap() = value;

                        // Run block
                        let this_ptr = this_ptr.as_deref_mut();
                        let statements = body.statements();

                        match self
                            .eval_stmt_block(global, caches, scope, this_ptr, statements, true)
                            .and_then(crate::scope::settle)
                        {
                            Ok(_) => (),
                            Err(err) => match *err {
                                ERR::LoopBreak(false, ..) => (),
                                ERR::LoopBreak(true, value, ..) => {
                                    result = value;
                                    break;
                                }
                                _ => return Err(err),
                            },
                        }
                    }
                }

                Ok(result)
            }

            // Continue/Break statement
            Stmt::BreakLoop(expr, options, pos) => {
                let is_break = options.intersects(ASTFlags::BREAK);

                let value = match expr {
                    Some(expr) => self.eval_expr(global, caches, scope, this_ptr, expr)?,
                    None => Dynamic::UNIT,
                };

                Err(ERR::LoopBreak(is_break, value, *pos).into())
            }

            // Try/Catch statement
            Stmt::TryCatch(x, ..) => {
                let FlowControl {
                    body,
                    expr: catch_var,
                    branch,
                } = &**x;

                match self.eval_stmt_block(
                    global,
                    caches,
                    scope,
                    this_ptr.as_deref_mut(),
                    body.statements(),
                    true,
                ) {
                    r @ Ok(_) => r,
                    Err(err) if err.is_pseudo_error() => Err(err),
                    Err(err) if !err.is_catchable() => Err(err),
                    Err(mut err) => {
                        let err_value = match err.unwrap_inner() {
                            // No error variable
                            _ if catch_var.is_unit() => Dynamic::UNIT,

                            ERR::ErrorRuntime(x, ..) => x.clone(),

                            _ => {
                                let mut err_map = crate::Map::new();
                                let err_pos = err.take_position();

                                err_map.insert("message".into(), err.to_string().into());

                                if let Some(ref source) = global.source {
                                    err_map.insert("source".into(), source.into());
                                }

                                if !err_pos.is_none() {
                                    err_map.insert(
                                        "line".into(),
                                        (err_pos.line().unwrap() as INT).into(),
                                    );
                                    err_map.insert(
                                        "position".into(),
                                        (err_pos.position().unwrap_or(0) as INT).into(),
                                    );
                                }

                                err.dump_fields(&mut err_map);
                                err_map.into()
                            }
                        };

                        // Restore scope at end of block
                        defer! { scope if !catch_var.is_unit() => rewind; let orig_scope_len = scope.len(); }

                        if let Expr::Variable(x, ..) = catch_var {
                            // Guard against too many variables
                            if scope.len() >= self.max_variables() {
                                return Err(ERR::ErrorTooManyVariables(catch_var.position()).into());
                            }
                            scope.push(x.1.clone(), err_value);
                        }

                        let this_ptr = this_ptr.as_deref_mut();
                        let statements = branch.statements();

                        self.eval_stmt_block(global, caches, scope, this_ptr, statements, true)
                            .map(|_| Dynamic::UNIT)
                            .map_err(|result_err| match *result_err {
                                // Re-throw exception
                                ERR::ErrorRuntime(v, pos) if v.is_unit() => {
                                    err.set_position(pos);
                                    err
                                }
                                _ => result_err,
                            })
                    }
                }
            }

            // Throw value
            Stmt::Return(Some(expr), options, pos) if options.intersects(ASTFlags::BREAK) => self
                .eval_expr(global, caches, scope, this_ptr, expr)
                .and_then(|v| Err(ERR::ErrorRuntime(v.flatten(), *pos).into())),

            // Empty throw
            Stmt::Return(None, options, pos) if options.intersects(ASTFlags::BREAK) => {
                Err(ERR::ErrorRuntime(Dynamic::UNIT, *pos).into())
            }

            // Return value
            Stmt::Return(Some(expr), .., pos) => self
                .eval_expr(global, caches, scope, this_ptr, expr)
                .and_then(|v| Err(ERR::Return(v.flatten(), *pos).into())),

            // Empty return
            Stmt::Return(None, .., pos) => Err(ERR::Return(Dynamic::UNIT, *pos).into()),

            // Import statement
            Stmt::Import(x, _pos) => {
                use crate::ModuleResolver;

                let (expr, export) = &**x;

                // Guard against too many modules
                if global.num_modules_loaded >= self.max_modules() {
                    return Err(ERR::ErrorTooManyModules(*_pos).into());
                }

                let v = self.eval_expr(global, caches, scope, this_ptr, expr)?;

                let path = v.try_cast_result::<crate::ImmutableString>().map_err(|v| {
                    self.make_type_mismatch_err::<crate::ImmutableString>(
                        v.type_name(),
                        expr.position(),
                    )
                })?;

                let path_pos = expr.start_position();

                let resolver = global.embedded_module_resolver.clone();

                let module = resolver
                    .as_ref()
                    .and_then(
                        |r| match r.resolve_raw(self, global, scope, &path, path_pos) {
                            Err(err) if matches!(*err, ERR::ErrorModuleNotFound(..)) => None,
                            result => Some(result),
                        },
                    )
                    .or_else(|| {
                        match self
                            .module_resolver()
                            .resolve_raw(self, global, scope, &path, path_pos)
                        {
                            Err(err) if matches!(*err, ERR::ErrorModuleNotFound(..)) => None,
                            result => Some(result),
                        }
                    })
                    .unwrap_or_else(|| {
                        Err(ERR::ErrorModuleNotFound(path.to_string(), path_pos).into())
                    })?;

                let (export, must_be_indexed) = if export.is_empty() {
                    (self.const_empty_string(), false)
                } else {
                    (export.name.clone(), true)
                };

                if !must_be_indexed || module.is_indexed() {
                    global.push_import(export, module);
                } else {
                    // Index the module (making a clone copy if necessary) if it is not indexed
                    let mut m = crate::func::shared_take_or_clone(module);
                    m.build_index();
                    global.push_import(export, m);
                }

                global.num_modules_loaded += 1;

                Ok(Dynamic::UNIT)
            }

            // Export statement
            Stmt::Export(x, ..) => {
                use crate::ast::Ident;
                let (Ident { name, pos, .. }, Ident { name: alias, .. }) = &**x;
                // Mark scope variables as public
                scope.search(name).map_or_else(
                    || Err(ERR::ErrorVariableNotFound(name.to_string(), *pos).into()),
                    |index| {
                        let alias = if alias.is_empty() { name } else { alias };
                        scope.add_alias_by_index(index, alias.clone());
                        Ok(Dynamic::UNIT)
                    },
                )
            }

            // Share statement
            Stmt::Share(x) => {
                for (var, index) in &**x {
                    // Check the variable resolver, if any
                    if let Some(ref resolve_var) = self.resolve_var {
                        let orig_scope_len = scope.len();

                        let context =
                            EvalContext::new(self, global, caches, scope, this_ptr.as_deref_mut());
                        let resolved_var = resolve_var(
                            &var.name,
                            index.map_or(0, core::num::NonZeroUsize::get),
                            context,
                        );

                        if orig_scope_len != scope.len() {
                            // The scope is changed, always search from now on
                            global.always_search_scope = true;
                        }

                        match resolved_var {
                            // If resolved to a variable, skip it (because we cannot make it shared)
                            Ok(Some(_)) => continue,
                            Ok(None) => (),
                            Err(err) => return Err(err.fill_position(var.pos)),
                        }
                    }

                    // Search the scope
                    let index = index
                        .map(|n| scope.len() - n.get())
                        .or_else(|| scope.search(&var.name))
                        .ok_or_else(|| {
                            Box::new(ERR::ErrorVariableNotFound(var.name.to_string(), var.pos))
                        })?;

                    let val = scope.get_mut_by_index(index);

                    if !val.is_shared() {
                        // Replace the variable with a shared value.
                        *val = val.take().into_shared();
                    }
                }

                Ok(Dynamic::UNIT)
            }

            #[allow(unreachable_patterns)]
            _ => unreachable!("statement cannot be evaluated: {:?}", stmt),
        }
    }

    /// Evaluate a list of statements with no `this` pointer.
    /// This is commonly used to evaluate a list of statements in an [`AST`][crate::AST] or a script function body.
    #[inline(always)]
    pub(crate) fn eval_global_statements(
        &self,
        global: &mut GlobalRuntimeState,
        caches: &mut Caches,
        scope: &mut Scope,
        statements: &[Stmt],
        map_exit_to_return_value: bool,
    ) -> RawrResult {
        // A scope frame never outlives the function body or script that
        // made it (D558 tier 1).
        self.eval_stmt_block(global, caches, scope, None, statements, false)
            .and_then(crate::scope::settle)
            .or_else(|err| match *err {
                ERR::Return(out, ..) => crate::scope::settle(out),
                ERR::Exit(out, ..) if map_exit_to_return_value => Ok(out),
                ERR::LoopBreak(..) => {
                    unreachable!("no outer loop scope to break out of")
                }
                _ => Err(err),
            })
    }
}
