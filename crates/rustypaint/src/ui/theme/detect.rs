use super::Mode;

pub fn system() -> Option<Mode> {
    from_override().or_else(|| match dark_light::detect().ok()? {
        dark_light::Mode::Dark => Some(Mode::Dark),
        dark_light::Mode::Light => Some(Mode::Light),
        dark_light::Mode::Unspecified => None,
    })
}

fn from_override() -> Option<Mode> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_desktop_is_survivable() {
        assert!(matches!(system(), Some(Mode::Light | Mode::Dark) | None));
    }
}
