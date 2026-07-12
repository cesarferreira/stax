use super::{AppView, ControlKind, control_button};
use crate::theme::{SYSTEM_UI_FONT, Theme};
use gpui::{
    Context, Div, InteractiveElement as _, ParentElement as _, Stateful,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct WelcomeView {
    recent_repositories: Vec<PathBuf>,
    error: Option<String>,
    opening: Option<PathBuf>,
    recent_loading: bool,
}

impl WelcomeView {
    pub fn new(
        recent_repositories: Vec<PathBuf>,
        error: Option<String>,
        opening: Option<PathBuf>,
        recent_loading: bool,
    ) -> Self {
        Self {
            recent_repositories,
            error,
            opening,
            recent_loading,
        }
    }

    #[cfg(test)]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    pub fn opening_path(&self) -> Option<&Path> {
        self.opening.as_deref()
    }

    pub fn render(&self, theme: Theme, cx: &mut Context<AppView>) -> Div {
        let open_button = control_button(
            "welcome-open-repository",
            "Open Repository",
            ControlKind::Primary,
            true,
            theme,
        )
        .on_click(cx.listener(|app, _, window, cx| {
            app.pick_repository(window, cx);
        }));

        let mut content = div()
            .w(px(620.0))
            .max_w_full()
            .flex()
            .flex_col()
            .gap_4()
            .p_8()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface_raised)
            .child(
                div()
                    .flex()
                    .items_end()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_2xl()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child("Stax"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.text_muted)
                                    .child("A native, read-only view of your stacked branches."),
                            ),
                    )
                    .child(open_button),
            );

        if let Some(path) = &self.opening {
            content = content.child(
                div()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.accent)
                    .bg(theme.surface_selected)
                    .text_sm()
                    .child(format!("Opening {}…", path.display())),
            );
        }

        if let Some(error) = &self.error {
            content = content.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.danger)
                    .text_sm()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.danger)
                            .child("Repository could not be opened"),
                    )
                    .child(error.clone())
                    .child(
                        div()
                            .text_color(theme.text_muted)
                            .child("Choose Open Repository or select another recent repository."),
                    ),
            );
        }

        content = content.child(
            div()
                .pt_2()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text_muted)
                .child("RECENT REPOSITORIES"),
        );

        if self.recent_loading {
            content = content.child(
                div()
                    .debug_selector(|| "recent-repositories-loading".into())
                    .px_3()
                    .py_4()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .text_sm()
                    .text_color(theme.text_muted)
                    .child("Loading recent repositories…"),
            );
        } else if self.recent_repositories.is_empty() {
            content = content.child(
                div()
                    .px_3()
                    .py_4()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .text_sm()
                    .text_color(theme.text_muted)
                    .child("No recent repositories yet."),
            );
        } else {
            content = content.children(
                self.recent_repositories
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(index, path)| recent_repository_row(index, path, theme, cx)),
            );
        }

        div()
            .debug_selector(|| "stax-welcome".into())
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .p_6()
            .font_family(SYSTEM_UI_FONT)
            .bg(theme.window)
            .text_color(theme.text)
            .child(content)
    }
}

fn recent_repository_row(
    index: usize,
    path: PathBuf,
    theme: Theme,
    cx: &mut Context<AppView>,
) -> Stateful<Div> {
    let name = repository_name(&path);
    let display_path = path.display().to_string();
    let open_path = path.clone();

    div()
        .id(("recent-repository", index))
        .focusable()
        .tab_index(index as isize + 1)
        .focus(move |style| style.border_color(theme.focus).bg(theme.surface_selected))
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .cursor_pointer()
        .hover(move |style| style.bg(theme.surface_selected))
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
        .child(
            div()
                .flex_none()
                .text_xs()
                .text_color(theme.accent)
                .child("Open"),
        )
        .on_click(cx.listener(move |app, _, window, cx| {
            app.open_repository(open_path.clone(), window, cx);
        }))
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
    fn recent_repository_name_uses_the_path_basename() {
        assert_eq!(repository_name(Path::new("/tmp/stax")), "stax");
    }
}
