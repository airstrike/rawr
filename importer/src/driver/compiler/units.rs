//! The compiler's answers to the engine's units of work (`engine.rs`). Each
//! answer reads the compiler and emits; none reads what the walk reached.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use rustc_data_structures::fingerprint::Fingerprint;
use rustc_data_structures::stable_hash::{StableHash, StableHasher};
use rustc_hir::def::{CtorOf, DefKind, Res};
use rustc_infer::infer::TyCtxtInferExt;
use rustc_middle::ty::{self, Ty, TyCtxt};
use rustc_span::def_id::{CrateNum, DefId};
use rustc_span::symbol::kw;
use rustc_span::Symbol;
use rustc_trait_selection::infer::InferCtxtExt;
use serde::Serialize;
use serde_json::value::RawValue;
use sha2::{Digest as _, Sha256};

use super::engine::{self, Emission, Fault, Hash, Key, Level, Unit};
use super::{
    associated_path_kind, crate_segment, defaulted_instance, hex, implementation_crate_authority,
    name_index, namespace, projected_alias_instance, safety, AssociatedDemandKind,
    AssociatedObligation, AssociatedPathKind, Binding, Budget, Builder, Diagnostic, ExternalCrate,
    Generics, ImplementationCrateAuthority, Invocation, Namespace, Polarity, Sysroot, Trait,
    TraitObligation,
};

/// A value as the protocol writes it, kept verbatim.
pub(super) fn raw<T: Serialize>(value: &T) -> Result<Box<RawValue>, String> {
    let text = serde_json::to_string(value)
        .map_err(|error| format!("failed to encode an extraction value: {error}"))?;
    RawValue::from_string(text).map_err(|error| format!("invalid extraction value: {error}"))
}

pub(super) fn fault(fault: Fault) -> String {
    match fault {
        Fault::Error(error) => error,
        Fault::Missing(unit) => format!("the extraction table lost its unit {unit}"),
    }
}

/// The handshake's sysroot and external crates for the crates a walk
/// reached. An external crate's artifacts are the loaded crate's.
pub(super) fn registered(
    commit: &str,
    target: &str,
    crates: &BTreeSet<engine::Crate>,
    loaded: &[ExternalCrate],
    budget: &mut Budget,
) -> Result<(Vec<Sysroot>, Vec<ExternalCrate>), String> {
    let mut sysroot = Vec::new();
    let mut external = Vec::new();
    for krate in crates {
        budget.items(1)?;
        if krate.sysroot {
            sysroot.push(Sysroot {
                compiler_commit: budget.owned(commit)?,
                selected_target: budget.owned(target)?,
                crate_name: budget.owned(&krate.crate_name)?,
                stable_crate_id: budget.owned(&krate.stable_crate_id)?,
            });
        } else {
            let loaded = loaded
                .iter()
                .find(|loaded| {
                    loaded.crate_name == krate.crate_name
                        && loaded.stable_crate_id == krate.stable_crate_id
                })
                .ok_or_else(|| "external crate was not loaded".to_owned())?;
            external.push(loaded.clone());
        }
    }
    sysroot.sort_unstable();
    external.sort_unstable();
    Ok((sysroot, external))
}

impl<'tcx, 'invocation, 'budget, 'diagnostic> Builder<'tcx, 'invocation, 'budget, 'diagnostic> {
    pub(super) fn new(
        tcx: TyCtxt<'tcx>,
        invocation: &'invocation Invocation,
        selected: CrateNum,
        budget: &'budget mut Budget,
        diagnostic: &'diagnostic mut Option<Diagnostic>,
    ) -> Self {
        Self {
            tcx,
            invocation,
            selected,
            emissions: Vec::new(),
            definitions: std::collections::HashMap::new(),
            obligations: std::collections::HashMap::new(),
            associated: std::collections::HashMap::new(),
            sysroot: BTreeMap::new(),
            external: BTreeMap::new(),
            implementation_index: std::collections::HashMap::new(),
            obligation_candidates: std::collections::HashMap::new(),
            reowned: None,
            instance_aliases: None,
            passing_aliases: std::collections::HashMap::new(),
            budget,
            diagnostic,
        }
    }
}

impl<'tcx> Builder<'tcx, '_, '_, '_> {
    /// Enumerate the crate-owned coherence domain without applying an impl
    /// to a consuming program's receiver.
    fn inventory(&mut self, root: DefId) -> Result<(), String> {
        let mut implementations = self.tcx.trait_impls_in_crate(root.krate).to_vec();
        implementations.sort_unstable_by_key(|id| self.tcx.def_path_hash(*id));
        let mut manifest = engine::model::Inventory {
            schema: 1,
            owner: format!("{:016x}", self.tcx.stable_crate_id(root.krate)),
            implementations: Vec::new(),
            associated: Vec::new(),
            unsupported: Vec::new(),
            inherent: Vec::new(),
            preludes: self.inventory_preludes()?,
            deref: self.inventory_deref()?,
        };
        for implementation in implementations {
            self.edge(1)?;
            let id = self.id(implementation, Namespace::Type)?;
            manifest.implementations.push(id.clone());
            match self.inventory_implementation(implementation)? {
                Some(associated) => manifest.associated.extend(associated),
                None => manifest.unsupported.push(id),
            }
        }
        manifest.implementations.sort_unstable();
        manifest.associated.sort_unstable();
        manifest.unsupported.sort_unstable();
        self.emissions.push(Emission::Inventory {
            inventory: manifest,
        });
        Ok(())
    }

    fn inventory_inherent(&mut self, owner: DefId) -> Result<(), String> {
        if owner.krate != self.selected
            || !matches!(
                self.tcx.def_kind(owner),
                DefKind::Struct | DefKind::Enum | DefKind::Union
            )
        {
            return Ok(());
        }
        let mut domain = engine::model::Inherent {
            owner: self.id(owner, Namespace::Type)?,
            implementations: Vec::new(),
            associated: Vec::new(),
        };
        for implementation in self.tcx.inherent_impls(owner) {
            self.edge(1)?;
            domain
                .implementations
                .push(self.id(*implementation, Namespace::Type)?);
            domain
                .associated
                .extend(
                    self.inventory_implementation(*implementation)?
                        .ok_or_else(|| {
                            "inherent implementation unexpectedly has reservation polarity"
                                .to_owned()
                        })?,
                );
        }
        domain.implementations.sort_unstable();
        domain.associated.sort_unstable();
        self.emissions
            .push(Emission::InherentInventory { inventory: domain });
        Ok(())
    }

    fn inventory_deref(&mut self) -> Result<Option<engine::model::Lookup>, String> {
        let Some(trait_id) = self.tcx.lang_items().deref_trait() else {
            return Ok(None);
        };
        let mut domains = Vec::new();
        for krate in self.tcx.crates(()).iter().copied() {
            if !self.is_sysroot_crate(krate)? {
                continue;
            }
            self.register_sysroot(krate)?;
            let mut implementations = Vec::new();
            for implementation in self.tcx.trait_impls_in_crate(krate) {
                self.edge(1)?;
                let Some(trait_ref) = self.tcx.impl_opt_trait_ref(*implementation) else {
                    continue;
                };
                if trait_ref.instantiate_identity().skip_norm_wip().def_id != trait_id {
                    continue;
                }
                let receiver = match self
                    .tcx
                    .type_of(*implementation)
                    .instantiate_identity()
                    .skip_norm_wip()
                    .kind()
                {
                    ty::Adt(definition, _) => engine::model::Receiver::Nominal(
                        self.id(definition.did(), Namespace::Type)?,
                    ),
                    ty::Bool
                    | ty::Char
                    | ty::Int(_)
                    | ty::Uint(_)
                    | ty::Float(_)
                    | ty::Str
                    | ty::Slice(_)
                    | ty::Array(..)
                    | ty::RawPtr(..)
                    | ty::Ref(..)
                    | ty::FnDef(..)
                    | ty::FnPtr(..)
                    | ty::Never
                    | ty::Tuple(_)
                    | ty::Dynamic(..) => engine::model::Receiver::Closed,
                    _ => engine::model::Receiver::Unresolved,
                };
                implementations.push(engine::model::LookupHeader {
                    implementation: self.id(*implementation, Namespace::Type)?,
                    receiver,
                });
            }
            implementations.sort_unstable();
            domains.push(engine::model::LookupDomain {
                root: self.id(krate.as_def_id(), Namespace::Type)?,
                implementations,
            });
        }
        domains.sort_unstable();
        Ok(Some(engine::model::Lookup {
            trait_id: self.id(trait_id, Namespace::Type)?,
            domains,
        }))
    }

    fn inventory_preludes(&mut self) -> Result<Vec<engine::model::Prelude>, String> {
        let mut preludes = Vec::new();
        let edition = self.invocation.edition.clone();
        let module_name = format!("rust_{edition}");
        for root in ["core", "std"] {
            let Some(krate) = self
                .tcx
                .crates(())
                .iter()
                .copied()
                .find(|krate| self.tcx.crate_name(*krate).as_str() == root)
            else {
                continue;
            };
            let mut module = krate.as_def_id();
            let mut found = true;
            for segment in ["prelude", &module_name] {
                match self
                    .tcx
                    .module_children(module)
                    .iter()
                    .find_map(|child| match child.res {
                        Res::Def(DefKind::Mod, id) if child.ident.name.as_str() == segment => {
                            Some(id)
                        }
                        _ => None,
                    }) {
                    Some(id) => module = id,
                    None => {
                        found = false;
                        break;
                    }
                }
            }
            if !found {
                continue;
            }
            self.register_sysroot(krate)?;
            let mut traits = Vec::new();
            for child in self.tcx.module_children(module) {
                let Res::Def(DefKind::Trait, id) = child.res else {
                    continue;
                };
                self.edge(1)?;
                self.register_sysroot(id.krate)?;
                let mut methods = BTreeSet::new();
                let mut seen = std::collections::HashSet::new();
                let mut pending = vec![id];
                while let Some(trait_id) = pending.pop() {
                    if !seen.insert(trait_id) {
                        continue;
                    }
                    self.edge(1)?;
                    for item in self.tcx.associated_items(trait_id).in_definition_order() {
                        if item.is_method() {
                            methods.insert(self.budget.owned(item.name().as_str())?);
                        }
                    }
                    for (clause, _) in self
                        .tcx
                        .explicit_super_predicates_of(trait_id)
                        .skip_binder()
                    {
                        if let ty::ClauseKind::Trait(bound) = clause.kind().skip_binder() {
                            pending.push(bound.def_id());
                        }
                    }
                }
                traits.push(engine::model::TraitNames {
                    id: self.id(id, Namespace::Type)?,
                    methods: methods.into_iter().collect(),
                });
            }
            traits.sort_unstable();
            traits.dedup();
            preludes.push(engine::model::Prelude {
                module: self.id(module, Namespace::Type)?,
                edition: edition.clone(),
                traits,
            });
        }
        preludes.sort_unstable();
        Ok(preludes)
    }

    fn inventory_implementation(
        &mut self,
        implementation: DefId,
    ) -> Result<Option<Vec<engine::model::Association>>, String> {
        self.implementation_base(implementation)?;
        let position = self
            .emissions
            .iter()
            .rposition(|emission| matches!(emission, Emission::Base { .. }))
            .ok_or_else(|| "intrinsic implementation base was not emitted".to_owned())?;
        if matches!(
            self.emissions[position],
            Emission::Base {
                implementation: None
            }
        ) {
            return Ok(None);
        }
        let id = self.id(implementation, Namespace::Type)?;
        let mut associations = Vec::new();
        let mut associated = Vec::new();
        for item in self
            .tcx
            .associated_items(implementation)
            .in_definition_order()
        {
            self.edge(1)?;
            let item_id = self.id(item.def_id, namespace(item.namespace()))?;
            let declaration = item
                .trait_item_def_id()
                .map(|declaration| self.id(declaration, namespace(item.namespace())))
                .transpose()?;
            self.ensure(item.def_id, namespace(item.namespace()))?;
            if let Some(declaration) = item.trait_item_def_id() {
                self.ensure(declaration, namespace(item.namespace()))?;
            }
            associations.push(engine::model::Association {
                implementation: id.clone(),
                item: item_id.clone(),
                declaration,
            });
            associated.push(item_id);
        }
        let Emission::Base {
            implementation: Some(record),
        } = &mut self.emissions[position]
        else {
            return Err(
                "intrinsic implementation base changed while acquiring its members".to_owned(),
            );
        };
        associated.sort_unstable();
        record.associated = associated;
        Ok(Some(associations))
    }

    /// The engine's key for a definition, remembered for later units.
    pub(super) fn key(&mut self, def_id: DefId, namespace: Namespace) -> Key {
        Key {
            hash: self.hash(def_id),
            namespace,
        }
    }

    pub(super) fn hash(&mut self, def_id: DefId) -> Hash {
        let hash = self.tcx.def_path_hash(def_id).to_raw_def_path_hash().0;
        self.definitions.insert(hash, def_id);
        hash
    }

    fn definition(&self, hash: &Hash) -> Result<DefId, String> {
        self.definitions.get(hash).copied().ok_or_else(|| {
            format!(
                "unit names definition {} this session never reached",
                engine::hex(hash)
            )
        })
    }

    /// A key no compilation session changes: the compiler's stable hash.
    fn stable<T: StableHash>(&self, value: &T) -> String {
        let fingerprint: Fingerprint = self.tcx.with_stable_hashing_context(|mut hcx| {
            let mut hasher = StableHasher::new();
            value.stable_hash(&mut hcx, &mut hasher);
            hasher.finish()
        });
        fingerprint.to_hex()
    }

    pub(super) fn obligation_key(&self, obligation: TraitObligation<'tcx>) -> String {
        let polarity: u8 = match obligation.polarity {
            ty::PredicatePolarity::Positive => 0,
            ty::PredicatePolarity::Negative => 1,
        };
        self.stable(&(
            (
                obligation.trait_id,
                obligation.self_ty,
                obligation.arguments,
            ),
            (polarity, obligation.associated, obligation.relevance),
            obligation.param_env,
        ))
    }

    fn associated_key(&self, obligation: AssociatedObligation<'tcx>) -> String {
        let kind: u8 = match obligation.kind {
            AssociatedDemandKind::SelectedMember => 0,
            AssociatedDemandKind::NominalSurface => 1,
        };
        self.stable(&(
            (kind, obligation.trait_item),
            obligation.arguments,
            obligation.param_env,
        ))
    }

    pub(super) fn request_implementation(
        &mut self,
        impl_id: DefId,
        relevance: DefId,
        associated: Option<AssociatedObligation<'tcx>>,
    ) -> Result<(), String> {
        let implementation = self.hash(impl_id);
        let owner = self.hash(relevance);
        let associated = associated.map(|obligation| {
            let key = self.associated_key(obligation);
            self.associated.insert(key.clone(), obligation);
            key
        });
        self.budget.items(1)?;
        self.emissions.push(Emission::Implementation {
            implementation,
            owner,
            associated,
        });
        Ok(())
    }

    pub(super) fn emit_definition(
        &mut self,
        def_id: DefId,
        namespace: Namespace,
        definition: &super::Definition,
        alias: Option<engine::AliasTarget>,
    ) -> Result<(), String> {
        self.budget.items(1)?;
        let key = self.key(def_id, namespace);
        self.emissions.push(Emission::Definition {
            key,
            id: definition.id.clone(),
            value: raw(definition)?,
            alias,
        });
        Ok(())
    }

    /// Where the type an alias passes its parameters on to lives.
    pub(super) fn alias_target(&mut self, target: DefId) -> Result<engine::AliasTarget, String> {
        Ok(engine::AliasTarget {
            target: self.key(target, Namespace::Type),
            krate: format!("{:016x}", self.tcx.stable_crate_id(target.krate)),
            selected: target.krate == self.selected,
            sysroot: self.is_sysroot_crate(target.krate)?,
            root: self.hash(target.krate.as_def_id()),
            segment: crate_segment(self.tcx, target.krate),
        })
    }

    /// An implementation's own fields and the requests reading them makes.
    fn implementation_base(&mut self, impl_id: DefId) -> Result<(), String> {
        match implementation_crate_authority(self.is_sysroot_crate(impl_id.krate)?) {
            ImplementationCrateAuthority::Sysroot => self.register_sysroot(impl_id.krate)?,
            ImplementationCrateAuthority::External => self.register_external(impl_id.krate)?,
        }
        let key = self.def_hash(impl_id)?;
        let implementation_id = self.id(impl_id, Namespace::Type)?;
        let compiler_self_ty = self
            .tcx
            .type_of(impl_id)
            .instantiate_identity()
            .skip_norm_wip();
        let compiler_trait_ref = self
            .tcx
            .impl_opt_trait_ref(impl_id)
            .map(|reference| reference.instantiate_identity().skip_norm_wip());
        self.breadcrumb(
            "begin",
            "implementation",
            "implementation-body",
            impl_id,
            Namespace::Type,
            "implementation",
        )?;
        let mut relevance = BTreeSet::new();
        for argument in compiler_self_ty.walk() {
            if let Some(inner) = argument.as_type()
                && let ty::Adt(definition, _) = inner.kind()
            {
                self.edge(1)?;
                self.ensure(definition.did(), Namespace::Type)?;
                self.budget.items(1)?;
                relevance.insert(self.id(definition.did(), Namespace::Type)?);
            }
        }
        let self_type = self.type_of(compiler_self_ty, impl_id, 0)?;
        let trait_ref = match compiler_trait_ref {
            None => Trait::Inherent,
            Some(reference) => {
                let trait_id: DefId = reference.def_id;
                self.edge(1)?;
                self.ensure(trait_id, Namespace::Type)?;
                self.budget.items(1)?;
                relevance.insert(self.id(trait_id, Namespace::Type)?);
                Trait::Trait {
                    id: self.id(trait_id, Namespace::Type)?,
                    arguments: self.arguments(reference.args, impl_id, 0)?,
                }
            }
        };
        let (impl_safety, polarity, const_impl) = match compiler_trait_ref {
            Some(_) => {
                let header = self.tcx.impl_trait_header(impl_id);
                let polarity = match header.polarity {
                    ty::ImplPolarity::Positive => Polarity::Positive,
                    ty::ImplPolarity::Negative => Polarity::Negative,
                    ty::ImplPolarity::Reservation => {
                        self.breadcrumb(
                            "complete",
                            "implementation",
                            "implementation-body",
                            impl_id,
                            Namespace::Type,
                            "implementation",
                        )?;
                        self.emissions.push(Emission::Base {
                            implementation: None,
                        });
                        return Ok(());
                    }
                };
                (
                    safety(header.safety),
                    polarity,
                    matches!(header.constness, rustc_hir::Constness::Const { .. }),
                )
            }
            None => (super::Safety::Safe, Polarity::Positive, false),
        };
        let generics = self.generics(impl_id)?;
        let span = self.span(self.tcx.def_span(impl_id))?;
        let origin = self.origin(self.tcx.def_span(impl_id))?;
        let record = engine::model::Implementation {
            key,
            id: implementation_id,
            relevance: relevance.into_iter().collect(),
            trait_ref: raw(&trait_ref)?,
            self_type: raw(&self_type)?,
            safety: raw(&impl_safety)?,
            polarity: raw(&polarity)?,
            const_impl,
            default_impl: self.tcx.defaultness(impl_id).is_default(),
            generics: raw(&generics)?,
            associated: Vec::new(),
            span: raw(&span)?,
            origin: raw(&origin)?,
        };
        self.budget.items(1)?;
        self.emissions.push(Emission::Base {
            implementation: Some(Box::new(record)),
        });
        self.breadcrumb(
            "complete",
            "implementation",
            "implementation-body",
            impl_id,
            Namespace::Type,
            "implementation",
        )?;
        Ok(())
    }

    /// An implementation's relevance to the definition that requested it.
    fn implementation_owner(&mut self, owner: DefId) -> Result<(), String> {
        let mut relevance = BTreeSet::new();
        self.insert_implementation_relevance_owner(owner, &mut relevance)?;
        self.emissions.push(Emission::Relevance {
            ids: relevance.into_iter().collect(),
        });
        Ok(())
    }

    /// The associated members one obligation selects in an implementation.
    fn implementation_associated(
        &mut self,
        impl_id: DefId,
        obligation: AssociatedObligation<'tcx>,
    ) -> Result<(), String> {
        let mut relevance = BTreeSet::new();
        if let Some(trait_item) = obligation.trait_item {
            self.insert_implementation_relevance_definition(trait_item, &mut relevance)?;
        }
        let implementation_id = self.id(impl_id, Namespace::Type)?;
        let compiler_trait_ref = self
            .tcx
            .impl_opt_trait_ref(impl_id)
            .map(|reference| reference.instantiate_identity().skip_norm_wip());
        let associated = self.expand_associated_obligations(
            impl_id,
            compiler_trait_ref,
            &[obligation],
            &implementation_id,
            false,
        )?;
        self.emissions.push(Emission::Relevance {
            ids: relevance.into_iter().collect(),
        });
        self.emissions
            .push(Emission::Associated { ids: associated });
        Ok(())
    }

    /// One demanded `Trait::member`: the member of each of the trait's impls
    /// for `&str`, as iced's `impl Widget<..> for &str` gives
    /// `Widget::_boxed("label")` that `column!` writes. Rawr's string is
    /// such a receiver, pinned as `&'static str`.
    fn trait_member(&mut self, trait_id: DefId, member: &str) -> Result<(), String> {
        let tcx = self.tcx;
        let name = Symbol::intern(member);
        let Some(trait_item) = tcx
            .associated_items(trait_id)
            .filter_by_name_unhygienic(name)
            .find(|item| item.is_fn())
            .map(|item| item.def_id)
        else {
            return Ok(());
        };
        let mut implementations = Vec::new();
        for impl_id in tcx.all_impls(trait_id) {
            self.budget.edge(1)?;
            let Some(reference) = tcx.impl_opt_trait_ref(impl_id) else {
                continue;
            };
            let reference = reference.instantiate_identity().skip_norm_wip();
            if matches!(
                reference.self_ty().kind(),
                ty::Ref(_, target, ty::Mutability::Not) if target.is_str()
            ) && tcx.impl_polarity(impl_id) == ty::ImplPolarity::Positive
            {
                implementations.push((impl_id, reference.args));
            }
        }
        implementations.sort_unstable_by_key(|(impl_id, _)| {
            tcx.def_path_hash(*impl_id).to_raw_def_path_hash().0
        });
        for (impl_id, arguments) in implementations {
            let obligation = AssociatedObligation {
                kind: AssociatedDemandKind::SelectedMember,
                trait_item: Some(trait_item),
                arguments: Some(arguments),
                param_env: tcx.param_env(impl_id),
            };
            self.request_implementation(impl_id, trait_id, Some(obligation))?;
        }
        Ok(())
    }

    /// One demanded `Type::member` that is no inherent member of the type:
    /// the member of every trait the type implements that declares it. A
    /// trait implemented for the type itself selects that member of its
    /// implementation. A blanket implementation (`impl<R: RngCore> Rng for
    /// R`) applies to the type as if the crate wrote `impl Rng for Type`.
    /// rustc proves the type implements the trait, under the type's own
    /// bounds for a generic type. Only a trait without generic parameters
    /// of its own is looked up.
    fn member(&mut self, adt: DefId, member: &str) -> Result<(), String> {
        let tcx = self.tcx;
        let generic = tcx.generics_of(adt).count() != 0;
        let name = Symbol::intern(member);
        let inherent = tcx.inherent_impls(adt).iter().any(|implementation| {
            tcx.associated_items(*implementation)
                .filter_by_name_unhygienic(name)
                .next()
                .is_some()
        });
        if inherent {
            return Ok(());
        }
        let self_ty = tcx.type_of(adt).instantiate_identity().skip_norm_wip();
        let param_env = tcx.param_env(adt);
        let mut traits = tcx
            .visible_traits()
            .filter(|trait_id| {
                tcx.associated_items(*trait_id)
                    .filter_by_name_unhygienic(name)
                    .any(|item| item.is_fn())
            })
            .collect::<Vec<_>>();
        traits
            .sort_unstable_by_key(|trait_id| tcx.def_path_hash(*trait_id).to_raw_def_path_hash().0);
        traits.dedup();
        // A generic type whose every parameter has a default is reached
        // at that instance, which its path names as a type: palette's
        // `impl<S, T: Stimulus> Default for Rgb<S, T>` applies to
        // `Rgb<Srgb, f32>`, though not to every `Rgb<S, T>`.
        let defaulted = generic.then(|| defaulted_instance(tcx, adt)).flatten();
        for trait_id in traits {
            self.edge(1)?;
            // A generic trait, as iced's `Widget<Message, Theme,
            // Renderer>`, is the type's own implementation of it, whose
            // trait arguments rustc proves where the bridge calls the
            // member: its provided `_boxed` is the type's.
            let trait_generic = tcx.generics_of(trait_id).count() != 1;
            let implements = |self_ty: Ty<'tcx>| {
                tcx.infer_ctxt()
                    .build(ty::TypingMode::non_body_analysis())
                    .type_implements_trait(trait_id, [self_ty], param_env)
                    .must_apply_modulo_regions()
            };
            // A trait the generic type implements for any arguments its
            // impl bounds, as iced's `impl<A: Renderer, B: Renderer>
            // Renderer for fallback::Renderer<A, B>`: an alias names the
            // instance, as `iced::Renderer` is `fallback::Renderer<wgpu,
            // tiny_skia>`, and rustc proves the bounds where the bridge
            // calls the member.
            let generally =
                || {
                    let mut found = false;
                    tcx.for_each_relevant_impl(trait_id, self_ty, |implementation| {
                        found |= tcx.impl_opt_trait_ref(implementation).is_some_and(|reference| {
                        matches!(
                            reference.instantiate_identity().skip_norm_wip().self_ty().kind(),
                            ty::Adt(definition, arguments) if definition.did() == adt
                                && arguments.types().all(|argument| {
                                    matches!(argument.kind(), ty::Param(_))
                                })
                        )
                    }) && tcx.impl_polarity(implementation) == ty::ImplPolarity::Positive;
                    });
                    found
                };
            let applies = trait_generic
                || implements(self_ty)
                || defaulted.is_some_and(implements)
                || (generic && generally());
            if !applies {
                continue;
            }
            let Some(trait_item) = tcx
                .associated_items(trait_id)
                .filter_by_name_unhygienic(name)
                .find(|item| item.is_fn())
                .map(|item| item.def_id)
            else {
                continue;
            };
            let mut candidates = Vec::new();
            tcx.for_each_relevant_impl(trait_id, self_ty, |implementation| {
                candidates.push(implementation);
            });
            let own = candidates.iter().copied().find(|implementation| {
                tcx.impl_opt_trait_ref(*implementation)
                    .is_some_and(|reference| {
                        matches!(
                            reference.instantiate_identity().skip_norm_wip().self_ty().kind(),
                            ty::Adt(definition, _) if definition.did() == adt
                        )
                    })
            });
            let arguments = match own {
                Some(implementation) if trait_generic => tcx
                    .impl_opt_trait_ref(implementation)
                    .map(|reference| reference.instantiate_identity().skip_norm_wip().args)
                    .unwrap_or_else(|| tcx.mk_args(&[self_ty.into()])),
                _ => tcx.mk_args(&[self_ty.into()]),
            };
            let obligation = AssociatedObligation {
                kind: AssociatedDemandKind::SelectedMember,
                trait_item: Some(trait_item),
                arguments: Some(arguments),
                param_env,
            };
            if let Some(implementation) = own {
                self.request_implementation(implementation, adt, Some(obligation))?;
                continue;
            }
            // A blanket implementation whose where-clause names a trait
            // the type implements itself, as iced's extension trait
            // `impl<T, M, Th, R> Widget<M, Th, R> for T where T:
            // core::Widget<M, Th, R>`, applies through that
            // implementation, which fixes the trait's arguments.
            if (trait_generic || generic)
                && self.apply_blanket_through(&candidates, adt, self_ty, obligation)?
            {
                continue;
            }
            // Any other blanket implementation of a generic trait is
            // left unresolved, as rustc would choose its arguments.
            if trait_generic {
                continue;
            }
            let blankets = candidates
                .iter()
                .copied()
                .filter(|implementation| {
                    tcx.generics_of(*implementation).count() == 1
                        && tcx
                            .impl_opt_trait_ref(*implementation)
                            .is_some_and(|reference| {
                                matches!(
                                    reference
                                        .instantiate_identity()
                                        .skip_norm_wip()
                                        .self_ty()
                                        .kind(),
                                    ty::Param(_)
                                )
                            })
                })
                .collect::<Vec<_>>();
            // Coherence admits one blanket implementation of a trait
            // for any one type. Several candidates whose bounds rustc
            // would have to choose between are left unresolved, as is
            // a blanket implementation for a generic type.
            if let [implementation] = blankets.as_slice()
                && !generic
            {
                self.apply_blanket(*implementation, adt, self_ty, obligation)?;
            }
        }
        Ok(())
    }

    /// The one blanket implementation among `candidates` (`impl<T, ..>
    /// Trait<..> for T where T: Base<..>`) whose where-clause on `T` names
    /// a trait `adt` implements itself (`impl<..> Base<..> for Adt<..>`),
    /// applied to `adt` through that implementation: the application has
    /// its generics and where-clauses, and its trait arguments fix every
    /// other parameter of the blanket implementation. rustc proves the
    /// blanket implementation's other bounds where the bridge calls the
    /// member. `false` when no candidate, or more than one, applies so.
    fn apply_blanket_through(
        &mut self,
        candidates: &[DefId],
        adt: DefId,
        self_ty: Ty<'tcx>,
        obligation: AssociatedObligation<'tcx>,
    ) -> Result<bool, String> {
        let tcx = self.tcx;
        let mut found = Vec::new();
        for &implementation in candidates {
            let Some(reference) = tcx.impl_opt_trait_ref(implementation) else {
                continue;
            };
            let reference = reference.instantiate_identity().skip_norm_wip();
            let ty::Param(blanket) = reference.self_ty().kind() else {
                continue;
            };
            let generics = tcx.generics_of(implementation);
            if generics.parent.is_some() {
                continue;
            }
            for (clause, _) in tcx.predicates_of(implementation).predicates {
                self.edge(1)?;
                let ty::ClauseKind::Trait(predicate) = clause.kind().skip_binder() else {
                    continue;
                };
                if predicate.polarity != ty::PredicatePolarity::Positive
                    || !matches!(predicate.self_ty().kind(), ty::Param(bounded) if bounded.index == blanket.index)
                {
                    continue;
                }
                let base_trait = predicate.def_id();
                let mut own = Vec::new();
                tcx.for_each_relevant_impl(base_trait, self_ty, |base| own.push(base));
                own.retain(|base| {
                    tcx.impl_opt_trait_ref(*base).is_some_and(|base| {
                        matches!(
                            base.instantiate_identity().skip_norm_wip().self_ty().kind(),
                            ty::Adt(definition, _) if definition.did() == adt
                        )
                    })
                });
                let [base] = own.as_slice() else {
                    continue;
                };
                let base_reference = tcx
                    .impl_opt_trait_ref(*base)
                    .ok_or_else(|| "base implementation lost its trait".to_owned())?
                    .instantiate_identity()
                    .skip_norm_wip();
                // Each parameter of the blanket implementation: its self
                // type is the base's, and each other is the base's trait
                // argument where the where-clause names it.
                let mut fixed = vec![None; generics.count()];
                fixed[blanket.index as usize] = Some(base_reference.self_ty().into());
                for (written, argument) in predicate
                    .trait_ref
                    .args
                    .iter()
                    .zip(base_reference.args.iter())
                    .skip(1)
                {
                    if let Some(written) = written.as_type()
                        && let ty::Param(parameter) = written.kind()
                        && let Some(slot) = fixed.get_mut(parameter.index as usize)
                    {
                        *slot = Some(argument);
                    }
                }
                let Some(arguments) = fixed.into_iter().collect::<Option<Vec<_>>>() else {
                    continue;
                };
                found.push((implementation, *base, tcx.mk_args(&arguments)));
            }
        }
        found.sort_unstable_by_key(|(implementation, base, _)| {
            (
                tcx.def_path_hash(*implementation).to_raw_def_path_hash().0,
                tcx.def_path_hash(*base).to_raw_def_path_hash().0,
            )
        });
        found.dedup_by_key(|(implementation, base, _)| (*implementation, *base));
        let [(implementation, base, arguments)] = found.as_slice() else {
            return Ok(false);
        };
        let (implementation, base, arguments) = (*implementation, *base, *arguments);
        let mut digest = Sha256::new();
        digest.update(b"rawr.blanket-through-application/v1");
        digest.update(tcx.def_path_hash(implementation).to_raw_def_path_hash().0);
        digest.update(tcx.def_path_hash(base).to_raw_def_path_hash().0);
        digest.update(tcx.def_path_hash(adt).to_raw_def_path_hash().0);
        let digest = digest.finalize();
        let key = hex(&digest[..16])?;
        let instantiated = tcx
            .impl_opt_trait_ref(implementation)
            .ok_or_else(|| "blanket implementation lost its trait".to_owned())?
            .instantiate(tcx, arguments)
            .skip_norm_wip();
        let identity = tcx
            .impl_opt_trait_ref(implementation)
            .map(|reference| reference.instantiate_identity().skip_norm_wip());
        match implementation_crate_authority(self.is_sysroot_crate(implementation.krate)?) {
            ImplementationCrateAuthority::Sysroot => self.register_sysroot(implementation.krate)?,
            ImplementationCrateAuthority::External => {
                self.register_external(implementation.krate)?;
            }
        }
        let mut id = self.id(base, Namespace::Type)?;
        id.def_path_hash = key.clone();
        let associated =
            self.expand_associated_obligations(implementation, identity, &[obligation], &id, true)?;
        self.ensure(adt, Namespace::Type)?;
        self.ensure(instantiated.def_id, Namespace::Type)?;
        let mut relevance = vec![
            self.id(adt, Namespace::Type)?,
            self.id(instantiated.def_id, Namespace::Type)?,
        ];
        relevance.sort_unstable();
        relevance.dedup();
        // The base implementation's parameters are the application's.
        let previous = self.reowned.replace((base, id.clone()));
        let parts = (|| {
            Ok::<_, String>((
                self.arguments(instantiated.args, base, 0)?,
                self.type_of(instantiated.self_ty(), base, 0)?,
                self.generics(base)?,
            ))
        })();
        self.reowned = previous;
        let (trait_arguments, self_type, generics) = parts?;
        let header = tcx.impl_trait_header(implementation);
        let record = engine::model::Implementation {
            key,
            id,
            relevance,
            trait_ref: raw(&Trait::Trait {
                id: self.id(instantiated.def_id, Namespace::Type)?,
                arguments: trait_arguments,
            })?,
            self_type: raw(&self_type)?,
            safety: raw(&safety(header.safety))?,
            polarity: raw(&Polarity::Positive)?,
            const_impl: false,
            default_impl: false,
            generics: raw(&generics)?,
            associated,
            span: raw(&self.span(tcx.def_span(implementation))?)?,
            origin: raw(&self.origin(tcx.def_span(implementation))?)?,
        };
        self.budget.items(1)?;
        self.emissions.push(Emission::Application {
            implementation: Box::new(record),
        });
        Ok(true)
    }

    /// The blanket implementation `implementation` applied to the concrete
    /// type `adt`: an implementation of the trait for that type, with the
    /// demanded member and no generics of its own.
    fn apply_blanket(
        &mut self,
        implementation: DefId,
        adt: DefId,
        self_ty: Ty<'tcx>,
        obligation: AssociatedObligation<'tcx>,
    ) -> Result<(), String> {
        let tcx = self.tcx;
        let mut digest = Sha256::new();
        digest.update(b"rawr.blanket-application/v1");
        digest.update(tcx.def_path_hash(implementation).to_raw_def_path_hash().0);
        digest.update(tcx.def_path_hash(adt).to_raw_def_path_hash().0);
        let digest = digest.finalize();
        let key = hex(&digest[..16])?;
        let instantiated = tcx
            .impl_opt_trait_ref(implementation)
            .ok_or_else(|| "blanket implementation lost its trait".to_owned())?
            .instantiate(tcx, tcx.mk_args(&[self_ty.into()]))
            .skip_norm_wip();
        let identity = tcx
            .impl_opt_trait_ref(implementation)
            .map(|reference| reference.instantiate_identity().skip_norm_wip());
        match implementation_crate_authority(self.is_sysroot_crate(implementation.krate)?) {
            ImplementationCrateAuthority::Sysroot => self.register_sysroot(implementation.krate)?,
            ImplementationCrateAuthority::External => {
                self.register_external(implementation.krate)?
            }
        }
        let mut id = self.id(implementation, Namespace::Type)?;
        id.def_path_hash = key.clone();
        let associated =
            self.expand_associated_obligations(implementation, identity, &[obligation], &id, true)?;
        self.ensure(adt, Namespace::Type)?;
        self.ensure(instantiated.def_id, Namespace::Type)?;
        // The ledger keeps relevance in identity order.
        let mut relevance = vec![
            self.id(adt, Namespace::Type)?,
            self.id(instantiated.def_id, Namespace::Type)?,
        ];
        relevance.sort_unstable();
        relevance.dedup();
        let header = tcx.impl_trait_header(implementation);
        let record = engine::model::Implementation {
            key,
            id,
            relevance,
            trait_ref: raw(&Trait::Trait {
                id: self.id(instantiated.def_id, Namespace::Type)?,
                arguments: self.arguments(instantiated.args, adt, 0)?,
            })?,
            self_type: raw(&self.type_of(self_ty, adt, 0)?)?,
            safety: raw(&safety(header.safety))?,
            polarity: raw(&Polarity::Positive)?,
            const_impl: false,
            default_impl: false,
            generics: raw(&Generics {
                parameters: Vec::new(),
                predicates: Vec::new(),
            })?,
            associated,
            span: raw(&self.span(tcx.def_span(implementation))?)?,
            origin: raw(&self.origin(tcx.def_span(implementation))?)?,
        };
        self.budget.items(1)?;
        self.emissions.push(Emission::Application {
            implementation: Box::new(record),
        });
        Ok(())
    }

    /// A module's public children, as a demanded path and a public walk
    /// read them.
    fn module_children(&mut self, module: Hash) -> Result<Vec<engine::Child>, String> {
        let module = self.definition(&module)?;
        let mut children = Vec::new();
        for child in self.tcx.module_children(module) {
            if !child.vis.is_public() {
                continue;
            }
            self.edge(1)?;
            let landing = match child.res {
                Res::Def(kind, def_id) => self.landing(kind, def_id),
                _ => None,
            };
            let export = if matches!(child.res, Res::Def(DefKind::Ctor(CtorOf::Variant, _), _)) {
                None
            } else {
                Some(self.child_export(child)?)
            };
            children.push(engine::Child {
                name: self.budget.owned(child.ident.name.as_str())?,
                landing,
                export,
            });
        }
        Ok(children)
    }

    /// What a demanded path's segment landing on `def_id` selects, as the
    /// walk resolves it: every item a segment lands on, in any namespace,
    /// is extracted as a selected root, so a module on the path is named and
    /// a type brings its impl tables.
    fn landing(&mut self, kind: DefKind, def_id: DefId) -> Option<Result<engine::Landing, String>> {
        let tcx = self.tcx;
        // A re-exported variant, as `pub use Alignment::Center;`, selects
        // its enum.
        let (kind, def_id) = match kind {
            DefKind::Variant => (DefKind::Enum, tcx.parent(def_id)),
            DefKind::Ctor(CtorOf::Variant, _) => (DefKind::Enum, tcx.parent(tcx.parent(def_id))),
            _ => (kind, def_id),
        };
        let ns = kind.ns()?;
        Some((|| {
            let mut requests = Vec::new();
            let owner = self.compiler_definition_owner(def_id)?;
            requests.push(self.definition_request(
                def_id,
                namespace(ns),
                owner,
                Level::Selected,
            )?);
            // `pub type Duration = TimeDelta;` names the type with its
            // members: selecting the alias selects the type.
            let (def_id, kind) = match kind {
                DefKind::TyAlias => match tcx
                    .type_of(def_id)
                    .instantiate_identity()
                    .skip_norm_wip()
                    .kind()
                {
                    ty::Adt(definition, arguments) if arguments.is_empty() => {
                        let target = definition.did();
                        let owner = self.compiler_definition_owner(target)?;
                        requests.push(self.definition_request(
                            target,
                            Namespace::Type,
                            owner,
                            Level::Selected,
                        )?);
                        (target, tcx.def_kind(target))
                    }
                    // A generic alias, or one with fixed arguments, selects
                    // each type it and its defaults name, with their impl
                    // tables, as rustc proves their bounds. `Alias::member`
                    // is its type's member.
                    aliased => {
                        let defaults = tcx
                            .generics_of(def_id)
                            .own_params
                            .iter()
                            .filter_map(|parameter| parameter.default_value(tcx))
                            .filter_map(|default| {
                                default.instantiate_identity().skip_norm_wip().as_type()
                            })
                            .collect::<Vec<_>>();
                        // An alias of an associated type names the type it
                        // gives at its defaults (`projected_alias_instance`).
                        let instance = projected_alias_instance(tcx, def_id);
                        let named = tcx
                            .type_of(def_id)
                            .instantiate_identity()
                            .skip_norm_wip()
                            .walk()
                            .chain(defaults.into_iter().flat_map(|default| default.walk()))
                            .chain(instance.into_iter().flat_map(|instance| instance.walk()))
                            .filter_map(|argument| argument.as_type())
                            .filter_map(|ty| match ty.kind() {
                                ty::Adt(definition, _) => Some(definition.did()),
                                _ => None,
                            })
                            .collect::<Vec<_>>();
                        for target in named {
                            let owner = self.compiler_definition_owner(target)?;
                            requests.push(self.definition_request(
                                target,
                                Namespace::Type,
                                owner,
                                Level::Selected,
                            )?);
                        }
                        match aliased {
                            ty::Adt(definition, _) => {
                                (definition.did(), tcx.def_kind(definition.did()))
                            }
                            _ => (def_id, kind),
                        }
                    }
                },
                _ => (def_id, kind),
            };
            let next = match kind {
                DefKind::Mod => engine::Next::Module(self.hash(def_id)),
                DefKind::Struct | DefKind::Enum => engine::Next::Nominal(self.hash(def_id)),
                DefKind::Trait => engine::Next::Trait(self.hash(def_id)),
                _ => engine::Next::Other,
            };
            Ok(engine::Landing { requests, next })
        })())
    }

    /// What a public walk names at one child.
    fn child_export(
        &mut self,
        child: &rustc_middle::metadata::ModChild,
    ) -> Result<engine::ChildExport, String> {
        let tcx = self.tcx;
        // A re-exported variant, as `pub use Alignment::Center;`, names its
        // enum's variant: the export is the enum's, with the variant's name.
        let (res, variant) = match child.res {
            Res::Def(DefKind::Variant, variant) => {
                let owner = tcx.parent(variant);
                (
                    Res::Def(DefKind::Enum, owner),
                    Some(self.budget.owned(tcx.item_name(variant).as_str())?),
                )
            }
            res => (res, None),
        };
        let binding = if child.ident.name == kw::Underscore {
            Binding::AnonymousImport
        } else {
            Binding::Nameable
        };
        let target = match res {
            Res::Def(kind, def_id) => match kind.ns().map(namespace) {
                None => engine::ChildTarget::Unnamespaced {
                    error: format!("external reachable definition has no namespace: {kind:?}"),
                },
                Some(ns) => {
                    let error = if matches!(
                        associated_path_kind(tcx, def_id)?,
                        Some(AssociatedPathKind::AnonymousSupport(_))
                    ) {
                        Some(
                            "anonymous associated support cannot become a reachable export"
                                .to_owned(),
                        )
                    } else if binding == Binding::AnonymousImport
                        && (kind != DefKind::Trait
                            || child.reexport_chain.is_empty()
                            || !matches!(
                                child.reexport_chain.last(),
                                Some(rustc_middle::metadata::Reexport::Single(_))
                            ))
                    {
                        Some("anonymous import lacks exact trait single-import evidence".to_owned())
                    } else {
                        None
                    };
                    let level = if binding == Binding::AnonymousImport {
                        Level::Shape
                    } else {
                        Level::Selected
                    };
                    let request = self
                        .compiler_definition_owner(def_id)
                        .and_then(|owner| self.definition_request(def_id, ns, owner, level));
                    let module = (kind == DefKind::Mod).then(|| engine::Nested {
                        module: self.hash(def_id),
                        foreign: def_id.is_crate_root() && def_id.krate != self.selected,
                    });
                    engine::ChildTarget::Definition {
                        key: self.key(def_id, ns),
                        id: self.id(def_id, ns)?,
                        error,
                        display_path: self.definition_path(def_id)?,
                        request: Box::new(request),
                        module,
                    }
                }
            },
            Res::PrimTy(primitive) => engine::ChildTarget::Primitive {
                compiler_kind: self
                    .budget
                    .formatted(format_args!("primitive:{primitive:?}"))?,
            },
            other => engine::ChildTarget::Other {
                error: format!("external reachable unsupported compiler resolution: {other:?}"),
            },
        };
        let span = self.span(child.ident.span)?;
        let mut reexports = Vec::new();
        let mut hops = Vec::new();
        for reexport in &child.reexport_chain {
            // A names-only walk extracts nothing, so a hop through a crate
            // none of whose definitions were extracted registers it there.
            if let Some(hop) = reexport.id()
                && hop.krate != self.selected
            {
                hops.push(engine::Crate {
                    sysroot: self.is_sysroot_crate(hop.krate)?,
                    crate_name: tcx.crate_name(hop.krate).as_str().to_owned(),
                    stable_crate_id: format!("{:016x}", tcx.stable_crate_id(hop.krate)),
                });
            }
            reexports.push(self.reexport(*reexport)?);
        }
        Ok(engine::ChildExport {
            binding,
            variant,
            span,
            reexports,
            hops,
            target,
        })
    }
}

impl engine::Oracle for Builder<'_, '_, '_, '_> {
    fn unit(&mut self, unit: &Unit) -> Result<Rc<[Emission]>, Fault> {
        self.emissions.clear();
        let result = (|| match unit {
            Unit::Inventory(hash) => {
                let root = self.definition(hash)?;
                self.inventory(root)
            }
            Unit::InventoryShape(key) => {
                let def_id = self.definition(&key.hash)?;
                let owner = self.compiler_definition_owner(def_id)?;
                self.materialize_shape(def_id, key.namespace, owner)?;
                self.inventory_inherent(def_id)
            }
            Unit::Shape(key) => {
                let def_id = self.definition(&key.hash)?;
                let owner = self.compiler_definition_owner(def_id)?;
                self.materialize_shape(def_id, key.namespace, owner)
            }
            Unit::Implementations(hash) => {
                let def_id = self.definition(hash)?;
                self.expand_implementations(def_id)
            }
            Unit::Surface(hash) => {
                let def_id = self.definition(hash)?;
                self.expand_selected_surface(def_id)
            }
            Unit::Obligation { key } => {
                let obligation = *self.obligations.get(key).ok_or_else(|| {
                    "unit names an obligation this session never reached".to_owned()
                })?;
                // Only a parameter's processing reads `selected`, and the
                // engine asks for a parameter's unit only when selected.
                self.process_trait_obligation(obligation, true)
            }
            Unit::Base(hash) => {
                let impl_id = self.definition(hash)?;
                self.implementation_base(impl_id)
            }
            Unit::Owner { owner, .. } => {
                let owner = self.definition(owner)?;
                self.implementation_owner(owner)
            }
            Unit::Associated {
                implementation,
                obligation,
            } => {
                let impl_id = self.definition(implementation)?;
                let obligation = *self.associated.get(obligation).ok_or_else(|| {
                    "unit names an associated obligation this session never reached".to_owned()
                })?;
                self.implementation_associated(impl_id, obligation)
            }
            Unit::Member { owner, name, .. } => {
                let adt = self.definition(owner)?;
                self.member(adt, name)
            }
            Unit::TraitMember { owner, name, .. } => {
                let trait_id = self.definition(owner)?;
                self.trait_member(trait_id, name)
            }
        })();
        let emissions = std::mem::take(&mut self.emissions);
        result?;
        Ok(Rc::from(emissions))
    }

    fn module(&mut self, module: Hash) -> Result<Rc<[engine::Child]>, Fault> {
        Ok(Rc::from(self.module_children(module)?))
    }

    fn segment(&mut self, name: &str) -> Result<Option<engine::Segment>, Fault> {
        let tcx = self.tcx;
        let Some(krate) = tcx
            .crates(())
            .iter()
            .copied()
            .find(|krate| crate_segment(tcx, *krate) == name)
        else {
            return Ok(None);
        };
        Ok(Some(engine::Segment {
            root: self.hash(krate.as_def_id()),
            krate: format!("{:016x}", tcx.stable_crate_id(krate)),
        }))
    }

    fn index(&mut self) -> Result<Rc<[Box<RawValue>]>, Fault> {
        let index = name_index(self.tcx, self.selected.as_def_id(), self.budget)?;
        let mut values = Vec::new();
        for name in &index {
            values.push(raw(name)?);
        }
        Ok(Rc::from(values))
    }
}
