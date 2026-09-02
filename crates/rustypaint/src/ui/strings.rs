pub fn with_key(label: &str, key: &str) -> String {
    format!("{label} ({key})")
}

pub fn shift_key(key: &str) -> String {
    command_key(&format!("Shift+{key}"))
}

pub fn command_key(key: &str) -> String {
    let modifier = if cfg!(target_os = "macos") {
        "Command"
    } else {
        "Ctrl"
    };
    format!("{modifier}+{key}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_key_hint_reads_the_way_windows_writes_one() {
        assert_eq!(
            super::with_key(crate::i18n::undo(), "Ctrl+Z"),
            "Undo (Ctrl+Z)"
        );
    }

    #[test]
    fn command_hints_name_the_platform_modifier() {
        let expected = if cfg!(target_os = "macos") {
            "Command+Z"
        } else {
            "Ctrl+Z"
        };
        assert_eq!(super::command_key("Z"), expected);
    }
}
