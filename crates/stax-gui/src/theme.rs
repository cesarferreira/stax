use gpui::{Hsla, WindowAppearance, rgb};

pub const SYSTEM_UI_FONT: &str = ".SystemUIFont";
pub const MONOSPACE_FONT: &str = "Menlo";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub window: Hsla,
    pub surface: Hsla,
    pub surface_raised: Hsla,
    pub surface_selected: Hsla,
    pub border: Hsla,
    pub border_strong: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub accent: Hsla,
    pub accent_text: Hsla,
    pub focus: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub danger: Hsla,
    pub diff_addition: Hsla,
    pub diff_deletion: Hsla,
    pub diff_hunk: Hsla,
    pub disabled_surface: Hsla,
    pub disabled_text: Hsla,
}

impl Theme {
    pub fn light() -> Self {
        Self {
            window: rgb(0xf3f3f4).into(),
            surface: rgb(0xf9f9fa).into(),
            surface_raised: rgb(0xffffff).into(),
            surface_selected: rgb(0xe6eef9).into(),
            border: rgb(0xd7d7da).into(),
            border_strong: rgb(0xb8b8bd).into(),
            text: rgb(0x202124).into(),
            text_muted: rgb(0x62656a).into(),
            accent: rgb(0x2b67ae).into(),
            accent_text: rgb(0xffffff).into(),
            focus: rgb(0x1f72cf).into(),
            success: rgb(0x287a45).into(),
            warning: rgb(0x915d10).into(),
            danger: rgb(0xb13a36).into(),
            diff_addition: rgb(0x1f7a3f).into(),
            diff_deletion: rgb(0xb23832).into(),
            diff_hunk: rgb(0x6750a4).into(),
            disabled_surface: rgb(0xebebed).into(),
            disabled_text: rgb(0x85878c).into(),
        }
    }

    pub fn dark() -> Self {
        Self {
            window: rgb(0x202124).into(),
            surface: rgb(0x27282b).into(),
            surface_raised: rgb(0x2f3034).into(),
            surface_selected: rgb(0x263e5d).into(),
            border: rgb(0x414349).into(),
            border_strong: rgb(0x5a5d64).into(),
            text: rgb(0xf0f0f1).into(),
            text_muted: rgb(0xb0b3b8).into(),
            accent: rgb(0x7aadea).into(),
            accent_text: rgb(0x142338).into(),
            focus: rgb(0x83b7f2).into(),
            success: rgb(0x70c78a).into(),
            warning: rgb(0xe0b25d).into(),
            danger: rgb(0xf58a83).into(),
            diff_addition: rgb(0x79c991).into(),
            diff_deletion: rgb(0xf08a84).into(),
            diff_hunk: rgb(0xb8a5ed).into(),
            disabled_surface: rgb(0x34363a).into(),
            disabled_text: rgb(0x858990).into(),
        }
    }

    pub fn for_appearance(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Theme;
    use gpui::{Hsla, WindowAppearance};

    fn relative_luminance(color: Hsla) -> f32 {
        let color = color.to_rgb();
        let linear = |channel: f32| {
            if channel <= 0.04045 {
                channel / 12.92
            } else {
                ((channel + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
    }

    fn contrast_ratio(foreground: Hsla, background: Hsla) -> f32 {
        let foreground = relative_luminance(foreground);
        let background = relative_luminance(background);
        let (lighter, darker) = if foreground > background {
            (foreground, background)
        } else {
            (background, foreground)
        };
        (lighter + 0.05) / (darker + 0.05)
    }

    #[test]
    fn light_and_vibrant_light_use_the_light_graphite_theme() {
        assert_eq!(
            Theme::for_appearance(WindowAppearance::Light),
            Theme::light()
        );
        assert_eq!(
            Theme::for_appearance(WindowAppearance::VibrantLight),
            Theme::light()
        );
    }

    #[test]
    fn dark_and_vibrant_dark_use_the_dark_graphite_theme() {
        assert_eq!(Theme::for_appearance(WindowAppearance::Dark), Theme::dark());
        assert_eq!(
            Theme::for_appearance(WindowAppearance::VibrantDark),
            Theme::dark()
        );
    }

    #[test]
    fn semantic_status_and_diff_tokens_remain_distinct() {
        for theme in [Theme::light(), Theme::dark()] {
            assert_ne!(theme.text, theme.text_muted);
            assert_ne!(theme.success, theme.warning);
            assert_ne!(theme.warning, theme.danger);
            assert_ne!(theme.diff_addition, theme.diff_deletion);
            assert_ne!(theme.focus, theme.surface_selected);
        }
    }

    #[test]
    fn small_status_text_meets_wcag_aa_on_normal_and_selected_rows() {
        for (appearance, theme) in [("light", Theme::light()), ("dark", Theme::dark())] {
            for (status, foreground) in [
                ("accent", theme.accent),
                ("muted", theme.text_muted),
                ("success", theme.success),
                ("warning", theme.warning),
                ("danger", theme.danger),
            ] {
                for (surface, background) in [
                    ("normal", theme.surface),
                    ("selected", theme.surface_selected),
                ] {
                    let ratio = contrast_ratio(foreground, background);
                    let required_ratio = if appearance == "light" && status == "warning" {
                        4.6
                    } else {
                        4.5
                    };
                    assert!(
                        ratio >= required_ratio,
                        "{appearance} {status} text on {surface} surface has contrast {ratio:.2}:1; required {required_ratio:.1}:1"
                    );
                }
            }
        }
    }
}
