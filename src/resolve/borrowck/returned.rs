//! What a function returns, checked against its own signature's lifetimes
//! as rustc's region check does (T170).

#[cfg(test)]
use super::note_unlowered;
use super::{region, signature, Body, Conflict, Flow, Local, Origin, Path, Projection, LIFETIME};

/// A parameter's borrow that the function returns at a position of its
/// output whose lifetime it does not relate to as the output's type
/// requires (T170): rustc's E0621 for a parameter whose lifetime is elided
/// where the output's is named, and its "lifetime may not live long enough"
/// otherwise.
pub(super) fn parameter(body: &Body) -> Option<Conflict> {
    let own = body.own.as_ref()?;
    let (flows, reborrows) = without_reborrows(body);
    let mut walker = region::Walker::over(&flows, body.locals.len());
    for &returned in &body.returned {
        let unsaid = || {
            #[cfg(test)]
            note_unlowered(
                "a returned borrow whose lifetimes these facts do not tell apart",
                body.locals[returned.0 as usize].declaration,
            );
        };
        for (local, regions, receiver) in &own.parameters {
            let parameter = Returned {
                local: *local,
                regions,
                into: returned,
                output: &own.output,
            };
            let broken = parameter
                .held(&mut walker, &unsaid)
                .or_else(|| parameter.reborrowed(&mut walker, &reborrows, &unsaid));
            let Some((lifetime, output)) = broken else {
                continue;
            };
            let info = &body.locals[local.0 as usize];
            let value = body.locals[returned.0 as usize].declaration;
            let explicit = !receiver
                && matches!(lifetime, signature::Lifetime::Elided(_))
                && matches!(output, signature::Lifetime::Named(_));
            return Some(if explicit {
                Conflict {
                    code: "E0621",
                    message: format!("explicit lifetime required in the type of `{}`", info.name),
                    access: value,
                    borrow: info.declaration,
                }
            } else {
                Conflict {
                    code: LIFETIME,
                    message: "lifetime may not live long enough".to_string(),
                    access: value,
                    borrow: info.declaration,
                }
            });
        }
    }
    None
}

/// A parameter of a function's own signature, against one value the
/// function returns.
struct Returned<'a> {
    local: Local,
    regions: &'a signature::Regions,
    into: Local,
    output: &'a signature::Output,
}

impl<'a> Returned<'a> {
    /// The parameter's lifetime, and the output's, where the returned value
    /// holds the one at a position of the output that needs the other.
    fn held(
        &self,
        walker: &mut region::Walker<'_>,
        unsaid: &dyn Fn(),
    ) -> Option<(&'a signature::Lifetime, &'a signature::Lifetime)> {
        use signature::Relation;
        self.regions.iter().find_map(|(from, lifetime)| {
            let reached = region::reach_into(walker, self.local, *from, self.into);
            let Some(lifetime) = lifetime else {
                if !reached.is_empty() {
                    unsaid();
                }
                return None;
            };
            reached
                .into_iter()
                .find_map(|at| match self.output.relate(lifetime, at) {
                    Relation::Holds => None,
                    Relation::Broken(output) => Some((lifetime, output)),
                    Relation::Unsaid => {
                        unsaid();
                        None
                    }
                })
        })
    }

    /// As [`Returned::held`], through a reborrow of what the parameter
    /// holds (`&**x`, `&y[0]`, `self.name.as_str()`). The reborrow lasts
    /// within the lifetime of the reference it goes through, which these
    /// facts don't place: one of the parameter's that its place's local
    /// holds. It is broken where none of them would hold.
    fn reborrowed(
        &self,
        walker: &mut region::Walker<'_>,
        reborrows: &[(Local, Local)],
        unsaid: &dyn Fn(),
    ) -> Option<(&'a signature::Lifetime, &'a signature::Lifetime)> {
        use signature::Relation;
        for &(temporary, place) in reborrows {
            let reached = region::reach_into(walker, temporary, Path::ROOT, self.into);
            if reached.is_empty() {
                continue;
            }
            let mut held = Vec::new();
            for (from, lifetime) in self.regions {
                if !region::reach_into(walker, self.local, *from, place).is_empty() {
                    held.push(lifetime.as_ref());
                }
            }
            let Some(held) = held.into_iter().collect::<Option<Vec<_>>>() else {
                unsaid();
                continue;
            };
            let Some(&first) = held.first() else {
                continue;
            };
            for &at in &reached {
                let relations: Vec<_> = held
                    .iter()
                    .map(|lifetime| self.output.relate(lifetime, at))
                    .collect();
                match relations
                    .iter()
                    .find(|relation| **relation != Relation::Holds)
                {
                    None => {}
                    Some(Relation::Broken(output))
                        if relations
                            .iter()
                            .all(|relation| matches!(relation, Relation::Broken(_))) =>
                    {
                        return Some((first, output));
                    }
                    Some(_) if relations.contains(&Relation::Holds) => {}
                    Some(_) => unsaid(),
                }
            }
        }
        None
    }
}

/// The body's flows less each reborrow's of all of its place's local
/// (`&*r`, `&r.a`), which liveness reads but which would relate every
/// lifetime that local holds to the output; and each such reborrow, as its
/// loan's hidden local and its place's local.
fn without_reborrows(body: &Body) -> (Vec<Flow>, Vec<(Local, Local)>) {
    let reborrows: Vec<(Local, Local)> = body
        .loans
        .iter()
        .filter(|loan| loan.place.projection.contains(&Projection::Deref))
        .map(|loan| (loan.temporary, loan.place.local))
        .collect();
    let mut through = vec![None; body.locals.len()];
    for &(temporary, place) in &reborrows {
        through[temporary.0 as usize] = Some(place);
    }
    let flows = body
        .flows
        .iter()
        .filter(|flow| {
            through[flow.into.0 as usize].is_none_or(|place| flow.from != Origin::local(place))
        })
        .copied()
        .collect();
    (flows, reborrows)
}
