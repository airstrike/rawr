//! One native view evaluation's borrow region.

/// A non-cloneable lowering region tied to one native view evaluation.
///
/// Native output cannot be widened beyond the region that borrowed its plan:
///
/// ```compile_fail
/// use rawr_iced::profile::runtime::Region;
///
/// fn escape<'view>(
///     region: &'view mut Region<'view>,
///     plan: &'view String,
/// ) -> iced::Element<'static, ()> {
///     region.lower(plan, |_context, text| iced::widget::text(text.as_str()).into())
/// }
/// ```
pub struct Region<'view> {
    context: &'view crate::widget::Context<'view>,
    _stamp: Stamp,
}

struct Stamp;

impl<'view> Region<'view> {
    /// Enter one view region with runner-authenticated lowering context.
    #[doc(hidden)]
    #[must_use]
    pub const fn new(context: &'view crate::widget::Context<'view>) -> Self {
        Self {
            context,
            _stamp: Stamp,
        }
    }

    /// Replay one borrowed symbolic plan into a fresh native element.
    pub fn lower<Message, Theme, Renderer, Plan, Replay>(
        &'view mut self,
        plan: &'view Plan,
        replay: Replay,
    ) -> iced::Element<'view, Message, Theme, Renderer>
    where
        Message: 'view,
        Theme: 'view,
        Renderer: iced::advanced::Renderer + 'view,
        Replay: FnOnce(
            &'view crate::widget::Context<'view>,
            &'view Plan,
        ) -> iced::Element<'view, Message, Theme, Renderer>,
    {
        replay(self.context, plan)
    }
}
