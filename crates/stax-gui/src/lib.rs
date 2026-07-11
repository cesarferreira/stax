mod views;

use gpui::{App, Application};
use std::path::PathBuf;

pub fn run(repository: Option<PathBuf>) {
    Application::new().run(move |cx: &mut App| {
        crate::views::open_initial_window(repository.clone(), cx);
        cx.activate(true);
    });
}
