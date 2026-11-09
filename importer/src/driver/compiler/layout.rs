//! rustc's layout of a struct, as far as a cycle of body statics of it
//! needs (D865): the offset of each address word of a pointer in it that
//! may hold a static's address, private fields and nested structs
//! included, and whether such a pointer is inside an enum variant or a
//! union, where its word is there only in some values.

use rustc_abi::{FieldsShape, Variants};
use rustc_middle::ty::layout::{LayoutCx, LayoutOf, TyAndLayout};
use rustc_middle::ty::{self, Ty, TyCtxt, TypeVisitableExt};
use rustc_span::def_id::DefId;

/// The most pointer words one struct's layout lists: a larger array of
/// pointers has no layout the runner takes.
const MOST: usize = 4096;

/// How deep a struct's fields nest before its layout is given up on.
const DEEPEST: usize = 64;

/// A struct's pointer words, as the runner takes them.
pub(super) struct Pointers {
    /// The struct's size in bytes.
    pub(super) size: u64,
    /// Each address word outside an enum variant or a union, by offset.
    pub(super) offsets: Vec<u64>,
    /// Whether a pointer is inside an enum variant or a union.
    pub(super) varied: bool,
}

/// The pointer words of the struct `def_id`, laid out without generics but
/// lifetimes; `None` for a struct generic over a type or a constant, or one
/// rustc can't lay out.
pub(super) fn of(tcx: TyCtxt<'_>, def_id: DefId) -> Option<Pointers> {
    let ty = tcx.type_of(def_id).instantiate_identity().skip_norm_wip();
    if ty.has_non_region_param() {
        return None;
    }
    let typing_env = ty::TypingEnv::post_analysis(tcx, def_id);
    let cx = LayoutCx::new(tcx, typing_env);
    let layout = cx.layout_of(ty).ok()?;
    if layout.is_unsized() {
        return None;
    }
    let mut pointers = Pointers {
        size: layout.size.bytes(),
        offsets: Vec::new(),
        varied: false,
    };
    walk(&cx, layout, 0, false, 0, &mut pointers)?;
    Some(pointers)
}

/// Add the pointer words of `layout`, a value at `offset`, inside an enum
/// variant or a union when `varied`.
fn walk<'tcx>(
    cx: &LayoutCx<'tcx>,
    layout: TyAndLayout<'tcx>,
    offset: u64,
    varied: bool,
    depth: usize,
    pointers: &mut Pointers,
) -> Option<()> {
    if depth > DEEPEST {
        return None;
    }
    match layout.ty.kind() {
        ty::Ref(_, pointee, _) | ty::RawPtr(pointee, _) => {
            if !may_be_static(*pointee) {
                return Some(());
            }
            if varied {
                pointers.varied = true;
                return Some(());
            }
            // A wide pointer's first field is its data pointer.
            let data = if layout.fields.count() == 0 {
                0
            } else {
                layout.fields.offset(0).bytes()
            };
            pointers.offsets.push(offset + data);
            return (pointers.offsets.len() <= MOST).then_some(());
        }
        ty::FnPtr(..) => return Some(()),
        _ => {}
    }
    if let Variants::Multiple { variants, .. } = &layout.variants {
        for index in variants.indices() {
            let variant = layout.for_variant(cx, index);
            fields(cx, variant, offset, true, depth, pointers)?;
        }
        return Some(());
    }
    let varied = varied || matches!(layout.fields, FieldsShape::Union(_));
    fields(cx, layout, offset, varied, depth, pointers)
}

fn fields<'tcx>(
    cx: &LayoutCx<'tcx>,
    layout: TyAndLayout<'tcx>,
    offset: u64,
    varied: bool,
    depth: usize,
    pointers: &mut Pointers,
) -> Option<()> {
    if let FieldsShape::Array { stride, count } = layout.fields {
        if count == 0 {
            return Some(());
        }
        let before = pointers.offsets.len();
        walk(cx, layout.field(cx, 0), offset, varied, depth + 1, pointers)?;
        let element: Vec<u64> = pointers.offsets[before..]
            .iter()
            .map(|word| word - offset)
            .collect();
        if element.is_empty() {
            return Some(());
        }
        let count = usize::try_from(count).ok()?;
        if element.len().checked_mul(count)? > MOST {
            return None;
        }
        for index in 1..count {
            let start = offset + stride.bytes() * u64::try_from(index).ok()?;
            pointers
                .offsets
                .extend(element.iter().map(|word| start + word));
        }
        return Some(());
    }
    for index in 0..layout.fields.count() {
        let field = layout.field(cx, index);
        let at = offset + layout.fields.offset(index).bytes();
        walk(cx, field, at, varied, depth + 1, pointers)?;
    }
    Some(())
}

/// Whether a pointer to a `pointee` may point at a static of a native
/// type: not one to text, numbers or other pointers, whose statics aren't
/// native places.
fn may_be_static(pointee: Ty<'_>) -> bool {
    match pointee.kind() {
        ty::Bool | ty::Char | ty::Int(_) | ty::Uint(_) | ty::Float(_) | ty::Str | ty::Never => {
            false
        }
        ty::Ref(..) | ty::RawPtr(..) | ty::FnPtr(..) | ty::FnDef(..) => false,
        ty::Slice(element) | ty::Array(element, _) => may_be_static(*element),
        ty::Tuple(members) => members.iter().any(may_be_static),
        _ => true,
    }
}
