//! What a function returns, or stores through a parameter, checked
//! against its own signature's lifetimes as rustc's region check does
//! (T170).

#[cfg(test)]
use super::note_unlowered;
use super::{
    region, signature, Body, Conflict, Flow, Local, Names, Origin, Path, Projection, Reborrow,
};
use crate::resolve::outlives;
use crate::{Position, Span};

/// Each parameter's borrow that the function returns at a position of its
/// output whose lifetime it does not relate to as the output's type
/// requires (T170): rustc's E0621 for a parameter whose lifetime is elided
/// where the output's is named, and its "lifetime may not live long enough"
/// otherwise. rustc reports one error for each lifetime that must outlive
/// another: of those it must outlive, the first the signature declares.
pub(super) fn parameter(body: &Body) -> Option<Conflict> {
    let own = body.own.as_ref()?;
    let (flows, reborrows, placed) = without_reborrows(body);
    let mut walker = region::Walker::over(&flows, body.locals.len());
    let mut reports: Vec<Reported<'_>> = Vec::new();
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
            let mut broken = parameter.held(&mut walker, &unsaid);
            if broken.is_empty() {
                broken.extend(
                    parameter
                        .through(&mut walker, &placed, &unsaid)
                        .or_else(|| parameter.reborrowed(&mut walker, &reborrows, &unsaid)),
                );
            }
            for (lifetime, required) in broken {
                let broken = Broken {
                    parameter: *local,
                    receiver: *receiver,
                    lifetime,
                    required,
                };
                let (longer, shorter) = broken.relates();
                let order = own.names.order(shorter);
                let earlier = reports
                    .iter()
                    .position(|reported| reported.longer == longer);
                if earlier.is_some_and(|earlier| reports[earlier].order <= order) {
                    continue;
                }
                let entry = Reported {
                    report: broken.report(body, own, value, outlives::Cause::Return),
                    longer,
                    order,
                    borrow: body.locals[local.0 as usize].declaration,
                };
                match earlier {
                    Some(earlier) => reports[earlier] = entry,
                    None => reports.push(entry),
                }
            }
        }
    }
    let borrow = reports.first()?.borrow;
    let reports = outlives::Reports::of(
        reports
            .into_iter()
            .map(|reported| reported.report)
            .collect(),
    )?;
    Some(Conflict {
        code: reports.first().code(),
        message: reports.first().message(),
        access: reports.first().returned(),
        borrow,
        tail: None,
        report: Some(Box::new(reports)),
    })
}

/// A report of a lifetime that must outlive another, which rustc reports
/// once for each such lifetime.
struct Reported<'a> {
    report: outlives::Report,
    /// The lifetime that must outlive the other.
    longer: &'a signature::Lifetime,
    /// Where the other is among the function's lifetimes ([`Names::order`]).
    order: (u8, Position, usize),
    /// The parameter's declaration.
    borrow: Span,
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
                .into_iter()
                .next()
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
            let report = broken.report(body, own, store.span, store.cause);
            let reports = outlives::Reports::of(vec![report])?;
            return Some(Conflict {
                code: reports.first().code(),
                message: reports.first().message(),
                access: store.span,
                borrow: body.locals[local.0 as usize].declaration,
                tail: None,
                report: Some(Box::new(reports)),
            });
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

impl<'a> Broken<'a> {
    /// The lifetime that must outlive the other, and the other.
    fn relates(&self) -> (&'a signature::Lifetime, &'a signature::Lifetime) {
        match self.required.direction {
            signature::Direction::Input => (self.lifetime, self.required.output),
            signature::Direction::Output => (self.required.output, self.lifetime),
        }
    }

    /// rustc's report of it, at `value`, of the function whose own
    /// signature is `own`.
    fn report(
        &self,
        body: &Body,
        own: &super::OwnSignature,
        value: Span,
        cause: outlives::Cause,
    ) -> outlives::Report {
        let names = &own.names;
        let info = &body.locals[self.parameter.0 as usize];
        match (self.lifetime, self.required.output) {
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
                let (longer, shorter) = self.relates();
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
                    named: names.named(longer, shorter, &own.parameters),
                }
            }
        }
    }
}

impl Names {
    /// Where rustc's region check reaches `lifetime` among the function's
    /// own: `'static`, then each named one where it is declared, then each
    /// elided one in order.
    fn order(&self, lifetime: &signature::Lifetime) -> (u8, Position, usize) {
        match lifetime {
            signature::Lifetime::Static => (0, Position::NONE, 0),
            signature::Lifetime::Named(name) => {
                let declared = self
                    .declared
                    .iter()
                    .find(|(declared, _)| declared == name)
                    .map_or(Position::NONE, |(_, at)| at.start());
                (1, declared, 0)
            }
            signature::Lifetime::Elided(number) => (2, Position::NONE, *number),
        }
    }

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
                Some(reference) => outlives::Region::Anonymous(*reference, 0),
                None => outlives::Region::Static,
            },
            signature::Lifetime::Static => outlives::Region::Static,
        }
    }
}

impl Names {
    /// rustc's suggestion of one named lifetime for `longer` and
    /// `shorter`, where each is the function's own and in its inputs: it
    /// is written at each lifetime the output elides, or else at
    /// `shorter`'s, then at `longer`'s, each with those elided below it.
    /// `None` where one of them is not a reference's.
    fn named(
        &self,
        longer: &signature::Lifetime,
        shorter: &signature::Lifetime,
        inputs: &[(Local, signature::Parameter, bool)],
    ) -> Option<Box<outlives::Named>> {
        use outlives::Place;
        let naming = &self.naming;
        let places = |numbers: std::ops::Range<usize>| -> Option<Vec<Place>> {
            let references = self.elided.get(numbers.clone())?;
            naming
                .elisions
                .get(numbers)?
                .iter()
                .zip(references)
                .map(|(elision, reference)| {
                    elision.reference.then_some(match elision.placeholder {
                        Some(placeholder) => Place::Over(placeholder),
                        None => Place::After(reference.ampersand),
                    })
                })
                .collect()
        };
        let input = |lifetime: &signature::Lifetime| match lifetime {
            signature::Lifetime::Elided(number) => {
                let below = naming.elisions.get(number - 1)?.below;
                places(number - 1..number + below)
            }
            // The function's own, which an input's type writes: the
            // receiver's type is `Self`'s, whose lifetimes it doesn't.
            signature::Lifetime::Named(_)
                if !matches!(
                    self.region(lifetime),
                    outlives::Region::Named { function: true, .. }
                ) =>
            {
                None
            }
            signature::Lifetime::Named(_) => inputs
                .iter()
                .any(|(_, parameter, receiver)| {
                    parameter.regions.iter().any(|(path, written)| {
                        written.as_ref() == Some(lifetime) && (!receiver || *path == Path::OWN)
                    })
                })
                .then(Vec::new),
            signature::Lifetime::Static => None,
        };
        let longer = input(longer)?;
        let shorter = input(shorter)?;
        let mut written = places(naming.input_elided..naming.elisions.len())?;
        if written.is_empty() {
            written = shorter;
        }
        for place in longer {
            if !written.contains(&place) {
                written.push(place);
            }
        }
        if written.is_empty() {
            return None;
        }
        let introduced = naming.first.is_none();
        if introduced {
            let at = naming.introduced?;
            written.push(Place::Parameter {
                at: at.at,
                listed: at.listed,
            });
        }
        Some(Box::new(outlives::Named {
            lifetime: naming.first.clone().unwrap_or_else(|| "a".into()),
            introduced,
            implemented: naming.implemented,
            places: written,
        }))
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
    /// Each of the parameter's lifetimes, and the output's, where the
    /// returned value holds the one at a position of the output that needs
    /// the other.
    fn held(
        &self,
        walker: &mut region::Walker<'_>,
        unsaid: &dyn Fn(),
    ) -> Vec<(&'a signature::Lifetime, signature::Required<'a>)> {
        let mut held = Vec::new();
        for (from, lifetime) in self.regions {
            let reached = region::reach_into(walker, self.local, *from, self.into);
            let Some(lifetime) = lifetime else {
                if !reached.is_empty() {
                    unsaid();
                }
                continue;
            };
            for at in reached {
                match self.output.broken(lifetime, at) {
                    Some(broken) => {
                        held.extend(broken.into_iter().map(|required| (lifetime, required)))
                    }
                    None => unsaid(),
                }
            }
        }
        held
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
