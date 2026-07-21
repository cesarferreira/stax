mod app;
mod changes_pane;
mod inspector_pane;
mod operation_overlay;
mod project_switcher;
mod stack_pane;
mod stack_topology;
mod text_input;
mod welcome;
mod workspace;

#[cfg(test)]
mod operation_tests;
#[cfg(test)]
mod tests;

use anyhow::Context as _;
use gpui::{App, AppContext as _, Bounds, Pixels, WindowBounds, WindowOptions, px, size};
use std::path::PathBuf;

use crate::preferences::{WindowSize, WindowSizePreferencesFile};
use app::AppServices;
pub use app::{AppView, ControlKind, activate_control, control_button, control_focus_style, init};
pub(crate) use app::{
    CheckoutSelected, CreateBranch, DeleteSelected, FocusStackSearch, MoveSelected,
    OpenPullRequest, OpenRepository, RedoLatest, RefreshRepository, RenameSelected,
    ReorderSelectedStack, RestackAll, RestackSelected, SubmitStack, ToggleChangesPane,
    ToggleInspectorPane, ToggleStackPane, UndoLatest,
};
pub use workspace::WorkspaceView;

#[cfg(test)]
pub use app::{
    AppModeKind, PaneMarkers, PickerFuture, RecentRepositoryStore, RepositoryPicker, RootLoadKind,
    SnapshotLoader,
};

pub fn open_initial_window(repository: Option<PathBuf>, cx: &mut App) -> anyhow::Result<()> {
    open_window_with_services(repository, AppServices::native(), cx)
}

fn open_window_with_services(
    repository: Option<PathBuf>,
    services: AppServices,
    cx: &mut App,
) -> anyhow::Result<()> {
    let saved_size = WindowSizePreferencesFile::default().load();
    let display_size = cx.primary_display().map(|display| display.bounds().size);
    let window_size = window_size_for_display(saved_size, display_size);
    let bounds = Bounds::centered(None, size(window_size.width, window_size.height), cx);
    let window_size_preferences = WindowSizePreferencesFile::default();
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(820.0), px(520.0))),
            ..Default::default()
        },
        move |window, cx| {
            let window_size_preferences = window_size_preferences.clone();
            window.on_window_should_close(cx, move |window, _| {
                if let Err(error) = window_size_preferences.save(WindowSize {
                    width: window.bounds().size.width,
                    height: window.bounds().size.height,
                }) {
                    eprintln!("stax-gui failed to save window size: {error:#}");
                }
                true
            });
            cx.new(|cx| AppView::new(repository, services, window, cx))
        },
    )
    .context("failed to open the Stax window")?;
    Ok(())
}

const DEFAULT_WINDOW_SIZE: WindowSize = WindowSize {
    width: px(1100.0),
    height: px(720.0),
};

fn window_size_for_display(
    saved: Option<WindowSize>,
    display_size: Option<gpui::Size<Pixels>>,
) -> WindowSize {
    let size = saved.unwrap_or(DEFAULT_WINDOW_SIZE);
    let Some(display_size) = display_size else {
        return size;
    };
    WindowSize {
        width: clamp_to_display(size.width, display_size.width),
        height: clamp_to_display(size.height, display_size.height),
    }
}

fn clamp_to_display(size: Pixels, display_size: Pixels) -> Pixels {
    if size > display_size {
        display_size
    } else {
        size
    }
}

#[cfg(test)]
mod window_size_tests {
    use super::{DEFAULT_WINDOW_SIZE, WindowSize, window_size_for_display};
    use gpui::{px, size};

    #[test]
    fn restored_size_is_clamped_to_the_current_display() {
        let restored = window_size_for_display(
            Some(WindowSize {
                width: px(1800.0),
                height: px(1200.0),
            }),
            Some(size(px(1440.0), px(900.0))),
        );

        assert_eq!(
            restored,
            WindowSize {
                width: px(1440.0),
                height: px(900.0),
            }
        );
    }

    #[test]
    fn default_size_is_used_when_no_window_size_has_been_saved() {
        assert_eq!(window_size_for_display(None, None), DEFAULT_WINDOW_SIZE);
    }
}
