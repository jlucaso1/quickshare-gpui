use gpui::prelude::FluentBuilder as _;
use gpui::*;

use crate::state::AppState;
use crate::views::content::ContentView;
use crate::views::header::HeaderView;
use crate::views::sidebar::SidebarView;

pub struct RootView {
    pub state: Entity<AppState>,
    header: Entity<HeaderView>,
    sidebar: Entity<SidebarView>,
    content: Entity<ContentView>,
}

impl RootView {
    pub fn new(state: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let header = cx.new(|_cx| HeaderView::new(state.clone()));
        let sidebar = cx.new(|_cx| SidebarView::new(state.clone()));
        let content = cx.new(|cx| ContentView::new(state.clone(), window, cx));

        Self {
            state,
            header,
            sidebar,
            content,
        }
    }
}

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let show_settings = self.state.read(cx).show_settings;

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(gpui::rgb(0xf8f9fa))
            .child(self.header.clone())
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_row()
                    .overflow_hidden()
                    .child(self.sidebar.clone())
                    .child(self.content.clone()),
            )
            .when(show_settings, |this| {
                this.child(super::settings_modal::render_settings_overlay(
                    self.state.clone(),
                    window,
                    cx,
                ))
            })
    }
}
