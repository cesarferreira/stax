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
use gpui::{App, AppContext as _, Bounds, WindowBounds, WindowOptions, px, size};
use std::path::PathBuf;

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
    let bounds = Bounds::centered(None, size(px(1100.0), px(720.0)), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(820.0), px(520.0))),
            ..Default::default()
        },
        move |window, cx| cx.new(|cx| AppView::new(repository, services, window, cx)),
    )
    .context("failed to open the Stax window")?;
    Ok(())
}
