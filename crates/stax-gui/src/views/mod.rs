use gpui::{
    App, Bounds, Context, Render, Window, WindowBounds, WindowOptions, div, prelude::*, px, size,
};
use std::path::PathBuf;

struct Placeholder {
    repository: Option<PathBuf>,
}

impl Render for Placeholder {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                self.repository
                    .as_ref()
                    .map(|path| format!("Stax · {}", path.display()))
                    .unwrap_or_else(|| "Stax".to_string()),
            )
    }
}

pub fn open_initial_window(repository: Option<PathBuf>, cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(1100.0), px(720.0)), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        move |_, cx| cx.new(|_| Placeholder { repository }),
    )
    .expect("open Stax window");
}
