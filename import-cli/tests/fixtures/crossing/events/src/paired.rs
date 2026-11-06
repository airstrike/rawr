//! An extension trait over every iterator, shaped like itertools'
//! `Itertools`: a supertrait of `Iterator` with a provided method and a
//! blanket impl for every `T: Iterator`, whose adaptor restarts a clone of
//! the iterator it is given, as itertools' `Product` does.

/// Extra methods for every iterator.
pub trait Paired: Iterator {
    /// Each item of `self` with each item of `other`, `other` restarted
    /// from a clone of itself for each of `self`'s items.
    fn cartesian_product<J>(self, other: J) -> Product<Self, J::IntoIter>
    where
        Self: Sized,
        Self::Item: Clone,
        J: IntoIterator,
        J::IntoIter: Clone,
    {
        let other = other.into_iter();
        Product {
            a: self,
            a_cur: None,
            b: other.clone(),
            b_orig: other,
        }
    }

    /// How many items are left.
    fn tally(self) -> usize
    where
        Self: Sized,
    {
        self.count()
    }
}

impl<T: Iterator + ?Sized> Paired for T {}

/// How many items `items` gives, as a crate function over any iterator.
pub fn length<I: Iterator>(items: I) -> usize {
    items.count()
}

/// The cartesian product of two iterators, as itertools' `Product`.
#[derive(Debug, Clone)]
pub struct Product<I, J>
where
    I: Iterator,
{
    a: I,
    a_cur: Option<Option<I::Item>>,
    b: J,
    b_orig: J,
}

impl<I, J> Iterator for Product<I, J>
where
    I: Iterator,
    J: Clone + Iterator,
    I::Item: Clone,
{
    type Item = (I::Item, J::Item);

    fn next(&mut self) -> Option<Self::Item> {
        let elt_b = match self.b.next() {
            None => {
                self.b = self.b_orig.clone();
                match self.b.next() {
                    None => return None,
                    Some(x) => {
                        self.a_cur = Some(self.a.next());
                        x
                    }
                }
            }
            Some(x) => x,
        };
        self.a_cur
            .get_or_insert_with(|| self.a.next())
            .as_ref()
            .map(|a| (a.clone(), elt_b))
    }
}
