#[derive(Clone, Debug)]
pub struct Input<'i> {
    chars: str::Chars<'i>,
}

impl<'i> Input<'i> {
    pub fn new_trim_c0_control_and_space(
        original_input: &'i str,
        vfn: Option<&dyn Fn(SyntaxViolation)>,
    ) -> Self {
        let input = original_input.trim_matches(c0_control_or_space);
        if let Some(vfn) = vfn {
            if input.len() < original_input.len() {
                vfn(SyntaxViolation::C0SpaceIgnored)
            }
            if input.chars().any(ascii_tab_or_new_line) {
                vfn(SyntaxViolation::TabOrNewlineIgnored)
            }
        }
        Input {
            chars: input.chars(),
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.clone().next().is_none()
    }

    #[inline]
    pub fn starts_with<P: Pattern>(&self, p: P) -> bool {
        p.split_prefix(&mut self.clone())
    }

    #[inline]
    pub fn split_prefix<P: Pattern>(&self, p: P) -> Option<Self> {
        let mut remaining = self.clone();
        if p.split_prefix(&mut remaining) {
            Some(remaining)
        } else {
            None
        }
    }

    #[inline]
    fn split_first(&self) -> (Option<char>, Self) {
        let mut remaining = self.clone();
        (remaining.next(), remaining)
    }

    #[inline]
    fn count_matching<F: Fn(char) -> bool>(&self, f: F) -> (u32, Self) {
        let mut count = 0;
        let mut remaining = self.clone();
        loop {
            let mut input = remaining.clone();
            if matches!(input.next(), Some(c) if f(c)) {
                remaining = input;
                count += 1;
            } else {
                return (count, remaining);
            }
        }
    }

    #[inline]
    fn next_utf8(&mut self) -> Option<(char, &'i str)> {
        loop {
            let utf8 = self.chars.as_str();
            match self.chars.next() {
                Some(c) => {
                    if !ascii_tab_or_new_line(c) {
                        return Some((c, &utf8[..c.len_utf8()]));
                    }
                }
                None => return None,
            }
        }
    }
}

pub trait Pattern {
    fn split_prefix(self, input: &mut Input) -> bool;
}

impl Pattern for char {
    fn split_prefix(self, input: &mut Input) -> bool {
        input.next() == Some(self)
    }
}

impl Pattern for &str {
    fn split_prefix(self, input: &mut Input) -> bool {
        for c in self.chars() {
            if input.next() != Some(c) {
                return false;
            }
        }
        true
    }
}

impl<F: FnMut(char) -> bool> Pattern for F {
    fn split_prefix(self, input: &mut Input) -> bool {
        input.next().map_or(false, self)
    }
}

impl Iterator for Input<'_> {
    type Item = char;
    fn next(&mut self) -> Option<char> {
        self.chars.by_ref().find(|&c| !ascii_tab_or_new_line(c))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.chars.as_str().len()))
    }
}
