//! Allocation sites for the closed stored selected-API schema.

use std::mem::size_of;

use super::{identity, ledger, model};

/// Conservative decoder work, distinct from the exact retained graph audit.
#[derive(Default)]
pub(crate) struct Shape {
    slots: usize,
    vectors: usize,
    boxes: usize,
    escaped: usize,
    depth: usize,
}

impl Shape {
    pub(crate) fn array_width(key: &[u8], depth: usize) -> usize {
        match key {
            // This sole fixed array is inline in StoredSelected, not a Vec.
            b"universe" if depth == 1 => 0,
            b"definitions" => size_of::<model::Definition>(),
            b"exports" => size_of::<ledger::Path>(),
            b"implementations" => size_of::<model::Implementation>(),
            b"scalar_predicate_proofs" => size_of::<model::ScalarPredicateProof>(),
            b"support" => size_of::<ledger::Support>(),
            b"anonymous" => size_of::<ledger::AnonymousImport>(),
            b"variants" => size_of::<model::Variant>(),
            b"public" => size_of::<model::Field>(),
            b"binders" => size_of::<model::Binder>(),
            b"inputs" | b"Tuple" => size_of::<model::Type>(),
            b"parameters" => size_of::<model::Parameter>(),
            b"predicates" | b"supertraits" => size_of::<model::Predicate>(),
            b"arguments" | b"captures" => size_of::<model::Argument>(),
            b"associated" | b"relevance" => size_of::<identity::Definition>(),
            b"aliases" => size_of::<ledger::Alias>(),
            b"reexports" => size_of::<ledger::Reexport>(),
            b"bounds" => size_of::<model::Bound>()
                .max(size_of::<model::ObjectBound>())
                .max(size_of::<model::Predicate>()),
            b"from" | b"to" | b"path" | b"display_path" | b"features" => size_of::<Box<str>>(),
            // Unknown or structurally misplaced arrays get no free allocation.
            // Semantic shape validation remains with the existing typed decoder.
            _ => Self::widest(),
        }
    }

    pub(crate) fn widest() -> usize {
        [
            size_of::<model::Definition>(),
            size_of::<ledger::Path>(),
            size_of::<model::Implementation>(),
            size_of::<model::ScalarPredicateProof>(),
            size_of::<ledger::Support>(),
            size_of::<ledger::AnonymousImport>(),
            size_of::<model::Variant>(),
            size_of::<model::Field>(),
            size_of::<model::Binder>(),
            size_of::<model::Type>(),
            size_of::<model::Parameter>(),
            size_of::<model::Predicate>(),
            size_of::<model::Bound>(),
            size_of::<model::ObjectBound>(),
            size_of::<model::Argument>(),
            size_of::<model::Signature>(),
            size_of::<identity::Definition>(),
            size_of::<ledger::Alias>(),
            size_of::<ledger::Reexport>(),
            size_of::<Box<str>>(),
        ]
        .into_iter()
        .max()
        .unwrap_or(0)
    }

    pub(crate) fn key(&mut self, key: &[u8]) {
        // All non-slice Box sites in model/identity/ledger are these Type and
        // Signature sites. `ty` may also be inline; counting it is conservative.
        let bytes = match key {
            b"Slice" | b"element" | b"ty" => size_of::<model::Type>(),
            b"FunctionPointer" => size_of::<model::Signature>(),
            _ => 0,
        };
        self.boxes = self.boxes.saturating_add(bytes);
    }

    pub(crate) fn array(&mut self, width: usize, elements: usize) {
        if width == 0 || elements == 0 {
            return;
        }
        self.slots = self.slots.saturating_add(width.saturating_mul(elements));
        // serde_core 1.0.229 VecVisitor starts empty: serde_json 1.0.151
        // SeqAccess has no size_hint. The pinned Rust RawVec::grow_amortized
        // doubles from this minimum. Sum every requested capacity, including
        // the optional into_boxed_slice shrink, to bound transient ownership.
        // A width may bound multiple concrete types on opposite sides of
        // RawVec's 1024-byte tier. Four covers either non-byte minimum even
        // when the widest concrete type itself would start at one element.
        let minimum = if width == 1 { 8 } else { 4 };
        let capacity = elements
            .checked_next_power_of_two()
            .unwrap_or(usize::MAX)
            .max(minimum);
        // The final capacity floor does not erase the smaller tier's earlier
        // requests: reaching four can allocate one, then two, then four.
        let first = if width == 1 { 8 } else { 1 };
        let growth = capacity.saturating_mul(2).saturating_sub(first);
        let shrink = if capacity == elements { 0 } else { elements };
        self.vectors = self
            .vectors
            .saturating_add(width.saturating_mul(growth.saturating_add(shrink)));
    }

    pub(crate) fn string(&mut self, encoded: &[u8]) {
        if encoded.contains(&b'\\') {
            self.escaped = self.escaped.max(encoded.len());
        }
    }

    pub(crate) fn depth(&mut self, depth: usize) {
        self.depth = self.depth.max(depth);
    }

    pub(crate) fn bytes(&self, strings: usize) -> (usize, usize) {
        // SliceRead borrows unescaped strings. Escapes use one reusable u8
        // scratch Vec; decoded bytes (including unicode reserve(4)) fit the
        // encoded string length. RawVec growth gives capacity <= max(8, 2E),
        // and the sum of allocation requests is less than twice that capacity.
        // Charge this conservative scratch work; it is not retained graph RAM.
        // Ignored unknown fields also use this scratch Vec as a delimiter
        // stack. Include the scanned depth even when no string is escaped.
        let scratch_length = self.escaped.max(self.depth);
        let scratch = if scratch_length == 0 {
            0
        } else {
            scratch_length.saturating_mul(2).max(8).saturating_mul(2)
        };
        let common = self.boxes.saturating_add(strings).saturating_add(scratch);
        (
            common.saturating_add(self.slots),
            common.saturating_add(self.vectors),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Shape;

    #[test]
    fn four_elements_include_the_large_element_growth_sequence() {
        for width in [2048, Shape::widest()] {
            let mut shape = Shape::default();
            shape.array(width, 4);
            let (logical, admission) = shape.bytes(0);
            assert_eq!(logical, 4 * width);
            assert_eq!(admission, (1 + 2 + 4) * width);
            assert!(admission >= (2 + 4) * width);
        }
    }
}
