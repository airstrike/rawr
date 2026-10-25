//! iced's tasks and streams are `Send` natively and not on wasm32, where a
//! browser runs one thread. [`Local`] carries one across the facade's `Send`
//! bounds there. Natively it is `Send` exactly when its value is.

pub(crate) struct Local<T>(pub(crate) T);

// SAFETY: wasm32 without the `atomics` target feature has one thread, so no
// value is ever sent to or shared with another.
#[cfg(all(target_arch = "wasm32", not(target_feature = "atomics")))]
unsafe impl<T> Send for Local<T> {}
#[cfg(all(target_arch = "wasm32", not(target_feature = "atomics")))]
unsafe impl<T> Sync for Local<T> {}

impl<S: iced::futures::Stream + Unpin> iced::futures::Stream for Local<S> {
    type Item = S::Item;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<S::Item>> {
        std::pin::Pin::new(&mut self.0).poll_next(context)
    }
}
