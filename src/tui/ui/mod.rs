pub(crate) mod interaction;

pub(crate) use interaction::{
    ConfirmId, DialogId, DialogInputId, InteractionKind, TextFieldId, ViewState,
};

pub(crate) struct RenderContext<'a> {
    pub(crate) theme: &'a crate::tui::theme::Theme,
    pub(crate) view: &'a mut ViewState,
}

impl<'a> RenderContext<'a> {
    pub(crate) fn new(theme: &'a crate::tui::theme::Theme, view: &'a mut ViewState) -> Self {
        Self { theme, view }
    }
}
