//! What a function returns, or stores through a parameter, checked
//! against its own signature's lifetimes as rustc's region check does
//! (T170).

#[cfg(test)]
use super::note_unlowered;
use super::{
    region, signature, Body, Conflict, Flow, Local, Names, Origin, Path, Projection, Reborrow,
};
use crate::resolve::outlives;

/// A parameter's borrow that the function returns at a position of its
/// output whose lifetime it does not relate to as the output's type
/// requires (T170): rustc's E0621 for a parameter whose lifetime is elided
/// where the output's is named, and its "lifetime may not live long enough"
/// otherwise.
pub(super) fn parameter(body: &Body) -> Option<Conflict> {
    let own = body.own.as_ref()?;
    let (flows, reborrows, placed) = without_reborrows(body);
    let mut walker = region::Walker::over(&flows, body.locals.len());
    for &returned in &body.returned {
        let value = body.locals[returned.0 as usize].declaration;
        let unsaid = || {
            #[cfg(test)]
            note_unlowered(
                "a returned borrow whose lifetimes these facts do not tell apart",
                value,
            );
        };
        for (local, parameter, receiver) in &own.parameters {
            let parameter = Returned {
                local: *local,
                regions: &parameter.regions,
                into: returned,
                output: &own.output,
            };
            let broken = parameter
                .held(&mut walker, &unsaid)
                .or_else(|| parameter.through(&mut walker, &placed, &unsaid))
                .or_else(|| parameter.reborrowed(&mut walker, &reborrows, &unsaid));
            if let Some((lifetime, required)) = broken {
                let broken = Broken {
                    parameter: *local,
                    receiver: *receiver,
                    lifetime,
                    required,
                };
                return Some(broken.conflict(body, &own.names, value, outlives::Cause::Return));
            }
        }
    }
    None
}

/// A parameter's borrow that the function stores through another
/// parameter (`v.push(x)`, `*r = x`, `h.x = x`) at a position whose
/// lifetime it does not outlive as the stored-into value's type requires.
pub(super) fn stored(body: &Body) -> Option<Conflict> {
    let own = body.own.as_ref()?;
    if body.stores.is_empty() {
        return None;
    }
    let (flows, reborrows, placed) = without_reborrows(body);
    let mut walker = region::Walker::over(&flows, body.locals.len());
    for store in &body.stores {
        let Some((_, target, _)) = own
            .parameters
            .iter()
            .find(|(local, ..)| *local == store.place)
        else {
            continue;
        };
        let unsaid = || {
            #[cfg(test)]
            note_unlowered(
                "a stored borrow whose lifetimes these facts do not tell apart",
                store.span,
            );
        };
        let sink = own.output.stored(target, &store.positions);
        for (local, parameter, receiver) in &own.parameters {
            if *local == store.place {
                continue;
            }
            let parameter = Returned {
                local: *local,
                regions: &parameter.regions,
                into: store.value,
                output: &sink,
            };
            let broken = parameter
                .held(&mut walker, &unsaid)
                .or_else(|| parameter.through(&mut walker, &placed, &unsaid))
                .or_else(|| parameter.reborrowed(&mut walker, &reborrows, &unsaid));
            let Some((lifetime, mut required)) = broken else {
                continue;
            };
            // A call through a `&mut` relates the lifetimes through what it
            // refers to, which rustc's notes name.
            if store.cause == outlives::Cause::Argument
                && required.invariance.is_none()
                && let [(value, at)] = store.positions[..]
                && value.is_root()
            {
                required.invariance = target.invariance_at(at);
            }
            let broken = Broken {
                parameter: *local,
                receiver: *receiver,
                lifetime,
                required,
            };
            return Some(broken.conflict(body, &own.names, store.span, store.cause));
        }
    }
    None
}

/// A parameter's lifetime that a value the body returns or stores does not
/// relate to as a signature's type requires.
struct Broken<'a> {
    parameter: Local,
    receiver: bool,
    lifetime: &'a signature::Lifetime,
    required: signature::Required<'a>,
}

impl Broken<'_> {
    /// rustc's report of it, at `value`.
    fn conflict(
        &self,
        body: &Body,
        names: &Names,
        value: crate::Span,
        cause: outlives::Cause,
    ) -> Conflict {
        let info = &body.locals[self.parameter.0 as usize];
        let report = match (self.lifetime, self.required.output) {
            (signature::Lifetime::Elided(number), signature::Lifetime::Named(output))
                if !self.receiver
                    && let Some(reference) = names.elided.get(number - 1) =>
            {
                outlives::Report::Explicit {
                    returned: value,
                    parameter: info.name.clone(),
                    lifetime: output.clone(),
                    reference: *reference,
                }
            }
            _ => {
                let (longer, shorter) = match self.required.direction {
                    signature::Direction::Input => (self.lifetime, self.required.output),
                    signature::Direction::Output => (self.required.output, self.lifetime),
                };
                outlives::Report::NotLongEnough {
                    returned: value,
                    cause,
                    item: names.item,
                    longer: names.region(longer),
                    shorter: names.region(shorter),
                    variance: self.required.invariance.and_then(|invariance| {
                        names
                            .variances
                            .iter()
                            .find(|(of, _)| of == invariance)
                            .map(|(_, variance)| variance.clone())
                    }),
                }
            }
        };
        Conflict {
            code: report.code(),
            message: report.message(),
            access: value,
            borrow: info.declaration,
            report: Some(Box::new(report)),
        }
    }
}

impl Names {
    /// The lifetime as rustc's report names it.
    fn region(&self, lifetime: &signature::Lifetime) -> outlives::Region {
        match lifetime {
            signature::Lifetime::Named(name) => {
                let declared = self
                    .declared
                    .iter()
                    .find(|(declared, _)| declared == name)
                    .map_or(crate::Span::NONE, |(_, at)| *at);
                outlives::Region::Named {
                    name: name.clone(),
                    declared,
                    function: self.name.is_none() || declared.start() > self.name.start(),
                }
            }
            signature::Lifetime::Elided(number) => match self.elided.get(number - 1) {
                Some(reference) => outlives::Region::Anonymous(*reference),
                None => outlives::Region::Static,
            },
            signature::Lifetime::Static => outlives::Region::Static,
        }
    }
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
    ) -> Option<(&'a signature::Lifetime, signature::Required<'a>)> {
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
                    Relation::Broken(required) => Some((lifetime, required)),
                    Relation::Unsaid => {
                        unsaid();
                        None
                    }
                })
        })
    }

    /// As [`Returned::held`], through a reborrow of what the parameter
    /// holds whose references these facts place (`&mut **x`, `self.a` of
    /// a `&mut` returned from a `&mut self`): it lasts within each of them,
    /// and is broken where one of them would not hold.
    fn through(
        &self,
        walker: &mut region::Walker<'_>,
        reborrows: &[Reborrow],
        unsaid: &dyn Fn(),
    ) -> Option<(&'a signature::Lifetime, signature::Required<'a>)> {
        use signature::Relation;
        for reborrow in reborrows
            .iter()
            .filter(|reborrow| reborrow.place == self.local)
        {
            // A place returned as it is reborrowed at the output's own region.
            let reached = if reborrow.value == self.into {
                vec![Path::OWN]
            } else {
                region::reach_into(walker, reborrow.value, Path::ROOT, self.into)
            };
            for &at in &reached {
                for position in &reborrow.through {
                    let lifetime = self
                        .regions
                        .iter()
                        .find(|(path, _)| path == position)
                        .and_then(|(_, lifetime)| lifetime.as_ref());
                    let Some(lifetime) = lifetime else {
                        unsaid();
                        continue;
                    };
                    match self.output.relate(lifetime, at) {
                        Relation::Holds => {}
                        Relation::Broken(required) => return Some((lifetime, required)),
                        Relation::Unsaid => unsaid(),
                    }
                }
            }
        }
        None
    }

    /// As [`Returned::held`], through a reborrow of what the parameter
    /// holds (`&**x`, `&y[0]`, `self.name.as_str()`) whose references
    /// these facts don't place. The reborrow lasts
    /// within the lifetime of the reference it goes through, which these
    /// facts don't place: one of the parameter's that its place's local
    /// holds. It is broken where none of them would hold.
    fn reborrowed(
        &self,
        walker: &mut region::Walker<'_>,
        reborrows: &[(Local, Local)],
        unsaid: &dyn Fn(),
    ) -> Option<(&'a signature::Lifetime, signature::Required<'a>)> {
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
                    Some(Relation::Broken(required))
                        if relations
                            .iter()
                            .all(|relation| matches!(relation, Relation::Broken(_))) =>
                    {
                        return Some((first, *required));
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
/// lifetime that local holds to the output; each such reborrow whose
/// references these facts don't place, as its loan's hidden local and its
/// place's local; and each one they place, with the body's returned ones.
fn without_reborrows(body: &Body) -> (Vec<Flow>, Vec<(Local, Local)>, Vec<Reborrow>) {
    let mut reborrows = Vec::new();
    let mut placed = body.returned_reborrows.clone();
    let mut through = vec![None; body.locals.len()];
    for loan in &body.loans {
        if !loan.place.projection.contains(&Projection::Deref) {
            continue;
        }
        through[loan.temporary.0 as usize] = Some(loan.place.local);
        match &loan.through {
            Some(references) => placed.push(Reborrow {
                value: loan.temporary,
                place: loan.place.local,
                through: references.clone(),
            }),
            None => reborrows.push((loan.temporary, loan.place.local)),
        }
    }
    let flows = body
        .flows
        .iter()
        .filter(|flow| {
            through[flow.into.0 as usize].is_none_or(|place| flow.from != Origin::local(place))
        })
        .copied()
        .collect();
    (flows, reborrows, placed)
}
