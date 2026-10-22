//! Structurally composed origin evidence for generated nominal source.

use std::ops::Range;

use crate::{Shared, Span};

use super::location;

// Nominal expression parsing is recursive. Keep each parser frame pointer-sized
// while the proof bundle grows independently on the heap.
#[derive(Clone, Default)]
pub(super) struct Fragment(Box<Parts>);

#[derive(Clone, Default)]
struct Parts {
    text: String,
    sites: Shared<Sites>,
    max_nesting: usize,
}

#[derive(Clone, Default)]
struct Sites {
    // Appended fragments remain relative child trees. No append walks or copies
    // the child's complete origin set; `finish` flattens the tree once.
    local: Vec<Origin>,
    children: Vec<Child>,
}

#[derive(Clone)]
struct Child {
    offset: usize,
    len: usize,
    sites: Shared<Sites>,
}

#[derive(Clone)]
struct Origin {
    generated: Range<usize>,
    original: Span,
    nesting: usize,
}

impl Fragment {
    pub(super) fn plain(text: impl Into<String>) -> Self {
        Self(Box::new(Parts {
            text: text.into(),
            sites: Shared::default(),
            max_nesting: 0,
        }))
    }

    #[cfg(test)]
    pub(super) fn exact(text: impl Into<String>, original: Span) -> Self {
        Self::plain(text).covered(original)
    }

    pub(super) fn join(fragments: impl IntoIterator<Item = Self>, separator: &str) -> Self {
        let mut joined = Self::default();
        for (index, fragment) in fragments.into_iter().enumerate() {
            if index > 0 {
                joined.push_text(separator);
            }
            joined.push(fragment);
        }
        joined
    }

    pub(super) fn call(name: impl AsRef<str>, arguments: impl IntoIterator<Item = Self>) -> Self {
        let mut call = Self::plain(name.as_ref());
        call.push_text("(");
        call.push(Self::join(arguments, ", "));
        call.push_text(")");
        call
    }

    pub(super) fn surrounded(self, prefix: &str, suffix: &str) -> Self {
        let mut surrounded = Self::plain(prefix);
        surrounded.push(self);
        surrounded.push_text(suffix);
        surrounded
    }

    #[cfg(test)]
    pub(super) fn covered(mut self, original: Span) -> Self {
        self.cover(original);
        self
    }

    pub(super) fn cover(&mut self, original: Span) {
        if self.0.text.is_empty() {
            return;
        }
        // Envelopes receive a larger rank than every range they contain. Both
        // translation consumers therefore select the smallest nesting rank.
        let nesting = self.0.max_nesting + 1;
        Shared::make_mut(&mut self.0.sites).local.push(Origin {
            generated: 0..self.0.text.len(),
            original,
            nesting,
        });
        self.0.max_nesting = nesting;
    }

    pub(super) fn push_text(&mut self, text: impl AsRef<str>) {
        self.0.text.push_str(text.as_ref());
    }

    pub(super) fn push(&mut self, mut fragment: Self) {
        let offset = self.0.text.len();
        self.0.text.push_str(&fragment.0.text);
        if !fragment.0.sites.is_empty() {
            Shared::make_mut(&mut self.0.sites).children.push(Child {
                offset,
                len: fragment.0.text.len(),
                sites: std::mem::take(&mut fragment.0.sites),
            });
        }
        self.0.max_nesting = self.0.max_nesting.max(fragment.0.max_nesting);
    }

    pub(super) fn split_off(&mut self, at: usize) -> Self {
        if at == 0 {
            let text = std::mem::take(&mut self.0.text);
            let sites = std::mem::take(&mut self.0.sites);
            let max_nesting = std::mem::take(&mut self.0.max_nesting);
            return Self(Box::new(Parts {
                text,
                sites,
                max_nesting,
            }));
        }
        if at == self.0.text.len() {
            return Self::default();
        }
        let text = self.0.text.split_off(at);
        let mut sites = Shared::unwrap_or_clone(std::mem::take(&mut self.0.sites));
        let suffix = sites.split_off(at);
        self.0.sites = Shared::new(sites);
        Self(Box::new(Parts {
            text,
            sites: Shared::new(suffix),
            max_nesting: self.0.max_nesting,
        }))
    }

    pub(super) fn truncate(&mut self, len: usize) {
        let _ = self.split_off(len);
    }

    pub(super) fn trim_end(&mut self) {
        let len = self.0.text.trim_end().len();
        self.truncate(len);
    }

    pub(super) fn clear(&mut self) {
        self.0.text.clear();
        Sites::release(std::mem::take(&mut self.0.sites));
        self.0.max_nesting = 0;
    }

    /// Replace each `from` with `to`, which has the same length, so every
    /// origin keeps its offsets.
    pub(super) fn replace_same_length(&mut self, from: &str, to: &str) {
        debug_assert_eq!(from.len(), to.len());
        if self.0.text.contains(from) {
            self.0.text = self.0.text.replace(from, to);
        }
    }

    pub(super) fn len(&self) -> usize {
        self.0.text.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.0.text.is_empty()
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0.text
    }

    #[cfg(test)]
    pub(super) fn origin_count(&self) -> usize {
        self.0.sites.origin_count()
    }

    pub(super) fn finish(mut self) -> (String, Vec<location::Output>) {
        let mut origins = Vec::new();
        Sites::collect(std::mem::take(&mut self.0.sites), &mut origins);
        origins.sort_by_key(|origin| {
            (
                origin.nesting,
                origin.generated.end - origin.generated.start,
                origin.generated.start,
                origin.original.start(),
                origin.original.end(),
            )
        });
        let locations = origins
            .into_iter()
            .map(|origin| location::Output {
                generated: origin.generated,
                original: origin.original,
                nesting: origin.nesting,
            })
            .collect();
        (std::mem::take(&mut self.0.text), locations)
    }
}

impl Drop for Fragment {
    fn drop(&mut self) {
        Sites::release(std::mem::take(&mut self.0.sites));
    }
}

impl Sites {
    fn is_empty(&self) -> bool {
        self.local.is_empty() && self.children.is_empty()
    }

    fn split_off(&mut self, at: usize) -> Self {
        let mut retained = Vec::with_capacity(self.local.len());
        let mut suffix = Vec::new();
        for mut origin in self.local.drain(..) {
            if origin.generated.end <= at {
                retained.push(origin);
            } else if origin.generated.start >= at {
                origin.generated.start -= at;
                origin.generated.end -= at;
                suffix.push(origin);
            } else {
                let mut tail = origin.clone();
                origin.generated.end = at;
                tail.generated = 0..tail.generated.end - at;
                retained.push(origin);
                suffix.push(tail);
            }
        }
        self.local = retained;

        let mut retained_children = Vec::with_capacity(self.children.len());
        let mut suffix_children = Vec::new();
        for mut child in self.children.drain(..) {
            let end = child.offset + child.len;
            if end <= at {
                retained_children.push(child);
            } else if child.offset >= at {
                child.offset -= at;
                suffix_children.push(child);
            } else {
                let cut = at - child.offset;
                let mut sites = Shared::unwrap_or_clone(child.sites);
                let tail = sites.split_off(cut);
                if !sites.is_empty() {
                    retained_children.push(Child {
                        offset: child.offset,
                        len: cut,
                        sites: Shared::new(sites),
                    });
                }
                if !tail.is_empty() {
                    suffix_children.push(Child {
                        offset: 0,
                        len: child.len - cut,
                        sites: Shared::new(tail),
                    });
                }
            }
        }
        self.children = retained_children;
        Self {
            local: suffix,
            children: suffix_children,
        }
    }

    fn collect(root: Shared<Self>, output: &mut Vec<Origin>) {
        let mut pending = vec![(0, root)];
        while let Some((offset, sites)) = pending.pop() {
            let sites = Shared::unwrap_or_clone(sites);
            output.extend(sites.local.into_iter().map(|mut origin| {
                origin.generated.start += offset;
                origin.generated.end += offset;
                origin
            }));
            pending.extend(
                sites
                    .children
                    .into_iter()
                    .map(|child| (offset + child.offset, child.sites)),
            );
        }
    }

    fn release(root: Shared<Self>) {
        // Deep widget expressions can build deep append trees. Reclaim unique
        // nodes iteratively instead of recursing through Shared's destructor.
        let mut pending = vec![root];
        while let Some(sites) = pending.pop() {
            if let Ok(sites) = Shared::try_unwrap(sites) {
                pending.extend(sites.children.into_iter().map(|child| child.sites));
            }
        }
    }

    #[cfg(test)]
    fn origin_count(&self) -> usize {
        let mut count = 0;
        let mut pending = vec![self];
        while let Some(sites) = pending.pop() {
            count += sites.local.len();
            pending.extend(sites.children.iter().map(|child| child.sites.as_ref()));
        }
        count
    }

    #[cfg(test)]
    fn node_count(&self) -> usize {
        1 + self
            .children
            .iter()
            .map(|child| child.sites.node_count())
            .sum::<usize>()
    }
}

impl From<String> for Fragment {
    fn from(text: String) -> Self {
        Self::plain(text)
    }
}

impl From<&str> for Fragment {
    fn from(text: &str) -> Self {
        Self::plain(text)
    }
}

#[cfg(test)]
mod tests {
    use super::Fragment;
    use crate::{Position, Span};

    #[test]
    fn structural_composition_offsets_and_prioritizes_nested_origins() {
        let inner_span = span(2, 3, 2, 8);
        let outer_span = span(1, 1, 3, 1);
        let mut nested = Fragment::exact("inner", inner_span);
        nested.push_text(")");
        let mut fragment = Fragment::plain("call(");
        fragment.push(nested);
        fragment.cover(outer_span);

        let (text, locations) = fragment.finish();

        assert_eq!(text, "call(inner)");
        assert_eq!(locations.len(), 2);
        assert_eq!(locations[0].generated, 5..10);
        assert_eq!(locations[0].original, inner_span);
        assert_eq!(locations[1].generated, 0..11);
        assert_eq!(locations[1].original, outer_span);
    }

    #[test]
    fn splitting_a_crossing_origin_preserves_both_structural_halves() {
        let original = span(1, 1, 1, 7);
        let mut fragment = Fragment::exact("prefix", original);

        let suffix = fragment.split_off(3);
        let (prefix_text, prefix) = fragment.finish();
        let (suffix_text, suffix) = suffix.finish();

        assert_eq!(prefix_text, "pre");
        assert_eq!(suffix_text, "fix");
        assert_eq!(prefix.len(), 1);
        assert_eq!(suffix.len(), 1);
        assert_eq!(prefix[0].original, original);
        assert_eq!(suffix[0].original, original);
    }

    #[test]
    fn splitting_a_nested_tail_rebases_sites_without_empty_children() {
        let original = span(4, 9, 4, 13);
        let mut fragment = Fragment::plain("let ");
        fragment.push(Fragment::exact("tail", original));

        let suffix = fragment.split_off(4);
        assert_eq!(fragment.origin_count(), 0);
        assert_eq!(fragment.0.sites.node_count(), 1);
        assert_eq!(suffix.origin_count(), 1);
        assert_eq!(suffix.0.sites.node_count(), 2);

        let (prefix_text, prefix) = fragment.finish();
        let (suffix_text, suffix) = suffix.finish();
        assert_eq!(prefix_text, "let ");
        assert!(prefix.is_empty());
        assert_eq!(suffix_text, "tail");
        assert_eq!(suffix[0].generated, 0..4);
        assert_eq!(suffix[0].original, original);
    }

    #[test]
    fn wide_and_deep_composition_retains_one_record_per_exact_cover() {
        let leaf_span = span(3, 5, 3, 9);
        let wrapper_span = span(2, 1, 4, 1);
        let mut wide = Fragment::default();

        for _ in 0..128 {
            wide.push(Fragment::exact("leaf", leaf_span));
        }
        for _ in 0..64 {
            wide.cover(wrapper_span);
        }

        assert_eq!(wide.len(), 512);
        assert_eq!(wide.origin_count(), 192);
        assert_eq!(wide.0.sites.node_count(), 129);
        let (_, wide_locations) = wide.finish();
        assert_eq!(wide_locations.len(), 192);

        let mut deep = Fragment::exact("leaf", leaf_span);
        for _ in 0..128 {
            let mut wrapper = Fragment::plain("(");
            wrapper.push(deep);
            wrapper.push_text(")");
            wrapper.cover(wrapper_span);
            deep = wrapper;
        }
        assert_eq!(deep.len(), 260);
        assert_eq!(deep.origin_count(), 129);
        assert_eq!(deep.0.sites.node_count(), 129);
        let (_, deep_locations) = deep.finish();
        assert_eq!(deep_locations.len(), 129);
    }

    #[test]
    fn dropping_a_deep_unfinished_fragment_releases_iteratively() {
        let original = span(7, 3, 7, 4);
        let mut fragment = Fragment::exact("x", original);

        for _ in 0..16_384 {
            let mut wrapper = Fragment::default();
            wrapper.push(fragment);
            wrapper.cover(original);
            fragment = wrapper;
        }

        assert_eq!(fragment.len(), 1);
        assert_eq!(fragment.origin_count(), 16_385);
        drop(fragment);
    }

    #[test]
    fn splitting_a_clone_does_not_mutate_the_shared_origin_tree() {
        let left = span(2, 5, 2, 9);
        let right = span(3, 5, 3, 10);
        let outer = span(1, 1, 4, 1);
        let mut fragment = Fragment::plain("pre:");
        fragment.push(Fragment::exact("left", left));
        fragment.push_text("|");
        fragment.push(Fragment::exact("right", right));
        fragment.cover(outer);
        let untouched = fragment.clone();

        let suffix = fragment.split_off(9);
        let (untouched_text, untouched_locations) = untouched.finish();
        let (prefix_text, prefix_locations) = fragment.finish();
        let (suffix_text, suffix_locations) = suffix.finish();

        assert_eq!(untouched_text, "pre:left|right");
        assert_eq!(untouched_locations.len(), 3);
        assert!(untouched_locations
            .iter()
            .any(|location| location.generated == (4..8) && location.original == left));
        assert!(untouched_locations
            .iter()
            .any(|location| location.generated == (9..14) && location.original == right));
        assert!(untouched_locations
            .iter()
            .any(|location| location.generated == (0..14) && location.original == outer));

        assert_eq!(prefix_text, "pre:left|");
        assert_eq!(prefix_locations.len(), 2);
        assert!(prefix_locations
            .iter()
            .any(|location| location.generated == (4..8) && location.original == left));
        assert!(prefix_locations
            .iter()
            .any(|location| location.generated == (0..9) && location.original == outer));

        assert_eq!(suffix_text, "right");
        assert_eq!(suffix_locations.len(), 2);
        assert!(suffix_locations
            .iter()
            .any(|location| location.generated == (0..5) && location.original == right));
        assert!(suffix_locations
            .iter()
            .any(|location| location.generated == (0..5) && location.original == outer));
    }

    const fn span(start_line: u16, start_column: u16, end_line: u16, end_column: u16) -> Span {
        Span::new(
            Position::new(start_line, start_column),
            Position::new(end_line, end_column),
        )
    }
}
