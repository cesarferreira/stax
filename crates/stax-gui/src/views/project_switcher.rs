use super::{AppView, ControlKind, activate_control, control_button, control_focus_style};
use crate::theme::Theme;
use gpui::{
    Context, Div, InteractiveElement as _, ParentElement as _, Stateful,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use std::path::{Path, PathBuf};

pub fn button(
    current_repository: &Path,
    open: bool,
    enabled: bool,
    theme: Theme,
    cx: &mut Context<AppView>,
) -> Stateful<Div> {
    let disclosure = if open { "▴" } else { "▾" };
    let button = control_button(
        "project-switcher-control",
        format!("{}  {disclosure}", repository_name(current_repository)),
        ControlKind::Secondary,
        enabled,
        theme,
    )
    .max_w(px(260.0))
    .tab_index(1);

    if enabled {
        activate_control(button, cx, |app, _window, cx| {
            app.toggle_project_switcher(cx);
        })
    } else {
        button
    }
}

pub fn menu(
    current_repository: &Path,
    recent_repositories: &[PathBuf],
    theme: Theme,
    cx: &mut Context<AppView>,
) -> Div {
    let alternatives = recent_repositories
        .iter()
        .filter(|path| path.as_path() != current_repository)
        .cloned()
        .collect::<Vec<_>>();

    let mut menu = div()
        .debug_selector(|| "project-switcher-menu".into())
        .absolute()
        .top(px(42.0))
        .left(px(16.0))
        .w(px(360.0))
        .max_w_full()
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .rounded_lg()
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.surface_raised)
        .shadow_lg()
        .child(
            div()
                .px_2()
                .py_1()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text_muted)
                .child("PROJECTS"),
        )
        .child(current_repository_row(current_repository, theme));

    if alternatives.is_empty() {
        menu = menu.child(
            div()
                .px_2()
                .py_2()
                .text_sm()
                .text_color(theme.text_muted)
                .child("No other recent projects."),
        );
    } else {
        menu = menu.children(
            alternatives
                .into_iter()
                .enumerate()
                .map(|(index, path)| repository_row(index, path, theme, cx)),
        );
    }

    let add = activate_control(
        control_button(
            "project-switcher-add",
            "+ Add Project…",
            ControlKind::Secondary,
            true,
            theme,
        )
        .tab_index(2),
        cx,
        |app, window, cx| app.pick_repository(window, cx),
    );

    menu.child(
        div()
            .mt_1()
            .pt_2()
            .border_t_1()
            .border_color(theme.border)
            .child(add),
    )
}

fn current_repository_row(path: &Path, theme: Theme) -> Div {
    div()
        .debug_selector(|| "project-switcher-current".into())
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .px_2()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.surface_selected)
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .truncate()
                        .text_sm()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(repository_name(path)),
                )
                .child(
                    div()
                        .truncate()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child(path.display().to_string()),
                ),
        )
        .child(div().text_xs().text_color(theme.accent).child("Current"))
}

fn repository_row(
    index: usize,
    path: PathBuf,
    theme: Theme,
    cx: &mut Context<AppView>,
) -> Stateful<Div> {
    let name = repository_name(&path);
    let display_path = path.display().to_string();
    let open_path = path.clone();
    let row = div()
        .id(("project-switcher-repository", index))
        .debug_selector(move || format!("project-switcher-repository-{index}"))
        .focusable()
        .tab_index(2)
        .focus(move |style| control_focus_style(style, theme).bg(theme.surface_selected))
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .px_2()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .cursor_pointer()
        .hover(move |style| style.bg(theme.surface_hover))
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .truncate()
                        .text_sm()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(name),
                )
                .child(
                    div()
                        .truncate()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child(display_path),
                ),
        )
        .child(div().text_xs().text_color(theme.accent).child("Open"));

    activate_control(row, cx, move |app, window, cx| {
        app.open_repository(open_path.clone(), window, cx);
    })
}

fn repository_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Repository")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::repository_name;
    use std::path::Path;

    #[test]
    fn project_name_uses_the_path_basename() {
        assert_eq!(repository_name(Path::new("/tmp/stax")), "stax");
    }
}
