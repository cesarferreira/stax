use crate::config::Config;
use anyhow::Result;
use colored::Colorize;

/// Print the current value of every `stax user` preference.
pub fn run_default() -> Result<()> {
    let config = Config::load_global()?;

    println!("{}", "User preferences:".blue().bold());
    println!(
        "  branch-prefix       {}",
        config.branch.prefix.as_deref().unwrap_or("(unset)")
    );
    println!("  branch-date         {}", bool_label(config.branch.date));
    println!("  branch-replacement  {}", config.branch.replacement);
    println!(
        "  editor              {}",
        config
            .editor
            .as_deref()
            .unwrap_or("(unset, falls back to $EDITOR)")
    );
    println!("  tips                {}", bool_label(config.ui.tips));
    println!(
        "  submit-body         {}",
        bool_label(config.submit.commit_messages_in_body)
    );

    Ok(())
}

pub fn branch_prefix(set: Option<String>, unset: bool) -> Result<()> {
    if let Some(prefix) = set {
        Config::set_branch_prefix(Some(prefix.clone()))?;
        print_set("branch.prefix", &prefix);
    } else if unset {
        Config::set_branch_prefix(None)?;
        print_unset("branch.prefix");
    } else {
        let config = Config::load_global()?;
        print_current(
            "branch.prefix",
            config.branch.prefix.as_deref().unwrap_or("(unset)"),
        );
    }
    Ok(())
}

pub fn branch_date(enable: bool, disable: bool) -> Result<()> {
    if enable || disable {
        Config::set_branch_date(enable)?;
        print_bool("branch.date", enable);
        warn_if_format_overrides_date()?;
    } else {
        let config = Config::load_global()?;
        print_current("branch.date", bool_label(config.branch.date));
    }
    Ok(())
}

pub fn branch_replacement(set: Option<String>, set_dash: bool, set_underscore: bool) -> Result<()> {
    let value = if set_dash {
        Some("-".to_string())
    } else if set_underscore {
        Some("_".to_string())
    } else {
        set
    };

    if let Some(value) = value {
        Config::set_branch_replacement(value.clone())?;
        print_set("branch.replacement", &value);
    } else {
        let config = Config::load_global()?;
        print_current("branch.replacement", &config.branch.replacement);
    }
    Ok(())
}

pub fn editor(set: Option<String>, unset: bool) -> Result<()> {
    if let Some(editor) = set {
        Config::set_editor(Some(editor.clone()))?;
        print_set("editor", &editor);
    } else if unset {
        Config::set_editor(None)?;
        print_unset("editor");
    } else {
        let config = Config::load_global()?;
        print_current(
            "editor",
            config
                .editor
                .as_deref()
                .unwrap_or("(unset, falls back to $EDITOR)"),
        );
    }
    Ok(())
}

pub fn tips(enable: bool, disable: bool) -> Result<()> {
    if enable || disable {
        Config::set_tips(enable)?;
        print_bool("ui.tips", enable);
    } else {
        let config = Config::load_global()?;
        print_current("ui.tips", bool_label(config.ui.tips));
    }
    Ok(())
}

pub fn submit_body(enable: bool, disable: bool) -> Result<()> {
    if enable || disable {
        Config::set_submit_commit_messages_in_body(enable)?;
        print_bool("submit.commit_messages_in_body", enable);
    } else {
        let config = Config::load_global()?;
        print_current(
            "submit.commit_messages_in_body",
            bool_label(config.submit.commit_messages_in_body),
        );
    }
    Ok(())
}

fn warn_if_format_overrides_date() -> Result<()> {
    let config = Config::load_global()?;
    if config.branch.format.is_some() {
        println!(
            "  {} branch.format is set, so branch.date has no effect until format is unset.",
            "!".yellow()
        );
    }
    Ok(())
}

fn bool_label(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn print_set(key: &str, value: &str) {
    println!("  {} Set {} = \"{}\"", "✓".green().bold(), key, value);
}

fn print_unset(key: &str) {
    println!("  {} Cleared {}", "✓".green().bold(), key);
}

fn print_bool(key: &str, enabled: bool) {
    println!(
        "  {} Set {} = {}",
        "✓".green().bold(),
        key,
        bool_label(enabled)
    );
}

fn print_current(key: &str, value: &str) {
    println!("  {} = {}", key, value);
}
