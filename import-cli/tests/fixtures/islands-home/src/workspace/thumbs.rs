//! Native: the workspace's thumbnail renderer, as inboard's
//! `thumbs::Renderer`, which the marked home screen is lent mutably and
//! lends on to its open wizard's `update`.

#[derive(Debug, Default)]
pub struct Renderer {
    pub drawn: usize,
}
