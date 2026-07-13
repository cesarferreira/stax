mod hydration;
mod operation;
pub mod preferences;
pub mod state;
pub mod theme;
mod views;

use gpui::{
    App, Application, KeyBinding, Menu, MenuItem, OsAction, PromptLevel, SystemMenuType, actions,
};
use std::path::PathBuf;

use crate::views::{
    CheckoutSelected, CreateBranch, DeleteSelected, FocusStackSearch, MoveSelected,
    OpenPullRequest, OpenRepository, RedoLatest, RefreshRepository, RenameSelected,
    ReorderSelectedStack, RestackAll, RestackSelected, SubmitStack, ToggleChangesPane,
    ToggleInspectorPane, ToggleStackPane, UndoLatest,
};

actions!(stax_app, [AboutStax, QuitStax]);

pub fn run(repository: Option<PathBuf>) {
    let application = Application::new();
    application.on_reopen(|cx| {
        if cx.windows().is_empty() {
            open_window_or_quit(None, cx);
        }
    });
    application.run(move |cx: &mut App| {
        crate::views::init(cx);
        cx.bind_keys([KeyBinding::new("cmd-q", QuitStax, None)]);
        cx.on_action(show_about);
        cx.on_action(quit);
        cx.set_menus(native_menus());
        open_window_or_quit(repository.clone(), cx);
        cx.activate(true);
    });
}

pub(crate) fn native_menus() -> Vec<Menu> {
    vec![
        Menu {
            name: "Stax".into(),
            items: vec![
                MenuItem::action("About Stax", AboutStax),
                MenuItem::separator(),
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Quit Stax", QuitStax),
            ],
        },
        Menu {
            name: "File".into(),
            items: vec![
                MenuItem::action("Open Repository…", OpenRepository),
                MenuItem::action("Refresh Repository", RefreshRepository),
            ],
        },
        Menu {
            name: "Edit".into(),
            items: vec![
                MenuItem::os_action("Undo", UndoLatest, OsAction::Undo),
                MenuItem::os_action("Redo", RedoLatest, OsAction::Redo),
            ],
        },
        Menu {
            name: "View".into(),
            items: vec![
                MenuItem::action("Search Stack", FocusStackSearch),
                MenuItem::separator(),
                MenuItem::action("Show/Hide Stack", ToggleStackPane),
                MenuItem::action("Show/Hide Changes", ToggleChangesPane),
                MenuItem::action("Show/Hide Inspector", ToggleInspectorPane),
            ],
        },
        Menu {
            name: "Branch".into(),
            items: vec![
                MenuItem::action("Checkout Selected", CheckoutSelected),
                MenuItem::action("Create Branch…", CreateBranch),
                MenuItem::action("Rename Branch…", RenameSelected),
                MenuItem::action("Delete Branch…", DeleteSelected),
                MenuItem::action("Move Branch…", MoveSelected),
            ],
        },
        Menu {
            name: "Stack".into(),
            items: vec![
                MenuItem::action("Reorder Stack…", ReorderSelectedStack),
                MenuItem::action("Restack Selected", RestackSelected),
                MenuItem::action("Restack All", RestackAll),
                MenuItem::separator(),
                MenuItem::action("Submit Stack…", SubmitStack),
                MenuItem::action("Open Pull Request", OpenPullRequest),
            ],
        },
    ]
}

fn show_about(_: &AboutStax, cx: &mut App) {
    let Some(window) = cx.active_window() else {
        return;
    };
    let _ = window.update(cx, |_, window, cx| {
        let _ = window.prompt(
            PromptLevel::Info,
            "Stax",
            Some(concat!(
                "Stacked Git branches and pull requests.\nVersion ",
                env!("CARGO_PKG_VERSION")
            )),
            &["OK"],
            cx,
        );
    });
}

fn quit(_: &QuitStax, cx: &mut App) {
    cx.quit();
}

fn open_window_or_quit(repository: Option<PathBuf>, cx: &mut App) {
    if let Err(error) = crate::views::open_initial_window(repository, cx) {
        eprintln!("stax-gui startup error: {error:#}");
        cx.quit();
    }
}
