pub mod preferences;
pub mod state;
pub mod theme;
mod views;

use gpui::{App, Application};
use std::path::PathBuf;

pub fn run(repository: Option<PathBuf>) {
    let application = Application::new();
    application.on_reopen(|cx| {
        if cx.windows().is_empty() {
            open_window_or_quit(None, cx);
        }
    });
    application.run(move |cx: &mut App| {
        crate::views::init(cx);
        open_window_or_quit(repository.clone(), cx);
        cx.activate(true);
    });
}

fn open_window_or_quit(repository: Option<PathBuf>, cx: &mut App) {
    if let Err(error) = crate::views::open_initial_window(repository, cx) {
        eprintln!("stax-gui startup error: {error:#}");
        cx.quit();
    }
}
