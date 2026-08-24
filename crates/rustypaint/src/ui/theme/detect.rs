use super::Mode;

use std::path::PathBuf;
use std::process::Command;

pub fn system() -> Option<Mode> {
    from_env()
        .or_else(portal)
        .or_else(|| {
            config_dir()
                .and_then(|d| read(d.join("kdeglobals")))
                .and_then(|c| kde(&c))
        })
        .or_else(gtk)
        .or_else(theme_env)
}

fn from_env() -> Option<Mode> {
    match std::env::var("RUSTYPAINT_THEME")
        .ok()?
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "dark" => Some(Mode::Dark),
        "light" => Some(Mode::Light),
        _ => None,
    }
}

fn portal() -> Option<Mode> {
    std::env::var_os("DBUS_SESSION_BUS_ADDRESS")?;

    const DEST: &str = "org.freedesktop.portal.Desktop";
    const OBJECT: &str = "/org/freedesktop/portal/desktop";
    const IFACE: &str = "org.freedesktop.portal.Settings";
    const NAMESPACE: &str = "org.freedesktop.appearance";
    const KEY: &str = "color-scheme";

    let calls: [(&str, Vec<String>); 3] = [
        (
            "busctl",
            vec![
                "--user".into(),
                "call".into(),
                DEST.into(),
                OBJECT.into(),
                IFACE.into(),
                "Read".into(),
                "ss".into(),
                NAMESPACE.into(),
                KEY.into(),
            ],
        ),
        (
            "gdbus",
            vec![
                "call".into(),
                "--session".into(),
                "--dest".into(),
                DEST.into(),
                "--object-path".into(),
                OBJECT.into(),
                "--method".into(),
                format!("{IFACE}.Read"),
                NAMESPACE.into(),
                KEY.into(),
            ],
        ),
        (
            "dbus-send",
            vec![
                "--session".into(),
                "--print-reply".into(),
                format!("--dest={DEST}"),
                OBJECT.into(),
                format!("{IFACE}.Read"),
                format!("string:{NAMESPACE}"),
                format!("string:{KEY}"),
            ],
        ),
    ];

    for (program, args) in calls {
        let Ok(output) = Command::new(program).args(&args).output() else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        if let Some(mode) = colour_scheme(&String::from_utf8_lossy(&output.stdout)) {
            return Some(mode);
        }
    }
    None
}

fn colour_scheme(reply: &str) -> Option<Mode> {
    let trailing: String = reply
        .trim()
        .trim_end_matches([')', ',', '>', ';', '"', '\''])
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect();
    match trailing
        .chars()
        .rev()
        .collect::<String>()
        .parse::<u32>()
        .ok()?
    {
        1 => Some(Mode::Dark),
        2 => Some(Mode::Light),
        _ => None,
    }
}

fn kde(contents: &str) -> Option<Mode> {
    let value = ini_value(contents, "ColorScheme")?;
    dark_named(&value).map(mode_from)
}

fn gtk() -> Option<Mode> {
    let base = config_dir()?;
    for version in ["gtk-4.0", "gtk-3.0"] {
        let Some(contents) = read(base.join(version).join("settings.ini")) else {
            continue;
        };
        if let Some(flag) = ini_value(&contents, "gtk-application-prefer-dark-theme") {
            return Some(mode_from(matches!(
                flag.as_str(),
                "1" | "true" | "TRUE" | "True"
            )));
        }
        if let Some(name) = ini_value(&contents, "gtk-theme-name")
            && let Some(dark) = dark_named(&name)
        {
            return Some(mode_from(dark));
        }
    }
    None
}

fn theme_env() -> Option<Mode> {
    for key in ["GTK_THEME", "QT_STYLE_OVERRIDE"] {
        if let Ok(value) = std::env::var(key)
            && let Some(dark) = dark_named(&value)
        {
            return Some(mode_from(dark));
        }
    }
    None
}

fn mode_from(dark: bool) -> Mode {
    if dark { Mode::Dark } else { Mode::Light }
}

fn dark_named(name: &str) -> Option<bool> {
    let lower = name.to_ascii_lowercase();
    if lower.contains("dark") {
        Some(true)
    } else if lower.contains("light") {
        Some(false)
    } else {
        None
    }
}

fn ini_value(contents: &str, key: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case(key)
            .then(|| value.trim().to_string())
    })
}

fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    Some(PathBuf::from(std::env::var_os("HOME")?).join(".config"))
}

fn read(path: PathBuf) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_helper_reply_shape_parses() {
        assert_eq!(colour_scheme("v u 1\n"), Some(Mode::Dark));
        assert_eq!(colour_scheme("(<uint32 1>,)\n"), Some(Mode::Dark));
        assert_eq!(colour_scheme("v u 2"), Some(Mode::Light));
        assert_eq!(colour_scheme("(<uint32 2>,)"), Some(Mode::Light));
        assert_eq!(
            colour_scheme("method return time=1.0 sender=:1.5\n   variant       uint32 1\n"),
            Some(Mode::Dark)
        );
    }

    #[test]
    fn no_preference_is_not_an_answer() {
        assert_eq!(colour_scheme("v u 0"), None);
        assert_eq!(colour_scheme("(<uint32 0>,)"), None);
    }

    #[test]
    fn rubbish_from_a_helper_is_survivable() {
        assert_eq!(colour_scheme(""), None);
        assert_eq!(
            colour_scheme("Failed to call method: no such interface"),
            None
        );
    }

    #[test]
    fn kde_says_it_in_its_scheme_name() {
        assert_eq!(kde("[General]\nColorScheme=BreezeDark\n"), Some(Mode::Dark));
        assert_eq!(
            kde("[General]\nColorScheme=BreezeLight\n"),
            Some(Mode::Light)
        );
        assert_eq!(kde("[General]\nColorScheme=Breeze\n"), None);
        assert_eq!(kde("[General]\nWidgetStyle=Breeze\n"), None);
    }

    #[test]
    fn a_theme_name_has_to_carry_the_word() {
        assert_eq!(dark_named("Adwaita-dark"), Some(true));
        assert_eq!(dark_named("Yaru:dark"), Some(true));
        assert_eq!(dark_named("Adwaita"), None);
        assert_eq!(dark_named("Breeze-Light"), Some(false));
        assert_eq!(dark_named(""), None);
    }

    #[test]
    fn ini_values_come_back_trimmed_and_case_insensitively() {
        let file = "[Settings]\n  gtk-application-prefer-dark-theme = 1  \n";
        assert_eq!(
            ini_value(file, "gtk-application-prefer-dark-theme"),
            Some("1".into())
        );
        assert_eq!(
            ini_value(file, "GTK-Application-Prefer-Dark-Theme"),
            Some("1".into())
        );
        assert_eq!(ini_value(file, "missing"), None);
        assert_eq!(ini_value("no equals sign here", "anything"), None);
    }

    #[test]
    fn nothing_in_the_chain_can_panic_on_an_empty_desktop() {
        assert!(matches!(
            system(),
            Some(Mode::Light) | Some(Mode::Dark) | None
        ));
    }
}
