use fluent_bundle::FluentResource;
use fluent_bundle::concurrent::FluentBundle;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use unic_langid::LanguageIdentifier;

pub use catalogue::*;
pub use fluent_bundle::FluentArgs as Args;

mod catalogue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "en-GB")]
    EnGb,
    #[serde(rename = "en-US")]
    EnUs,
    #[serde(rename = "ro-RO")]
    Ro,
}

impl Language {
    pub const ALL: [Language; 4] = [Language::Auto, Language::EnGb, Language::EnUs, Language::Ro];

    // Every locale but Automatic names itself, the way a language list is normally written.
    pub fn name(self) -> &'static str {
        match self {
            Language::Auto => language_auto(),
            Language::EnGb => "English (UK)",
            Language::EnUs => "English (US)",
            Language::Ro => "Română",
        }
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

struct Catalogue {
    language: Language,
    tag: &'static str,
    source: &'static str,
}

macro_rules! catalogue {
    ($language:expr, $tag:literal) => {
        Catalogue {
            language: $language,
            tag: $tag,
            source: include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../res/i18n/",
                $tag,
                ".ftl"
            )),
        }
    };
}

// The first entry is the source catalogue and the fallback for anything unmatched.
const CATALOGUES: &[Catalogue] = &[
    catalogue!(Language::EnGb, "en-GB"),
    catalogue!(Language::EnUs, "en-US"),
    catalogue!(Language::Ro, "ro-RO"),
];

struct Loaded {
    bundle: FluentBundle<FluentResource>,
    plain: HashMap<&'static str, String>,
}

static ACTIVE: AtomicUsize = AtomicUsize::new(0);
static LOADED: [OnceLock<Loaded>; CATALOGUES.len()] = [const { OnceLock::new() }; CATALOGUES.len()];

pub fn init(language: Language) {
    ACTIVE.store(index_for(language), Ordering::Relaxed);
}

pub fn lookup(key: &'static str) -> &'static str {
    loaded().plain.get(key).map_or(key, String::as_str)
}

pub fn format(key: &'static str, args: Args<'_>) -> String {
    resolve(&loaded().bundle, key, Some(&args)).unwrap_or_else(|| key.to_owned())
}

#[macro_export]
macro_rules! tr {
    ($key:literal $(, $name:ident = $value:expr)* $(,)?) => {{
        #[allow(unused_mut)]
        let mut args = $crate::i18n::Args::new();
        $(args.set(stringify!($name), $value);)*
        $crate::i18n::format($key, args)
    }};
}

fn loaded() -> &'static Loaded {
    let index = ACTIVE.load(Ordering::Relaxed);
    let slot: &'static OnceLock<Loaded> = &LOADED[index];
    slot.get_or_init(|| build(index))
}

fn build(index: usize) -> Loaded {
    let catalogue = &CATALOGUES[index];
    let langid: LanguageIdentifier = catalogue.tag.parse().expect("a catalogue tag parses");
    let resource = FluentResource::try_new(catalogue.source.to_owned())
        .map_err(|(_, errors)| errors)
        .expect("a catalogue parses");

    let mut bundle = FluentBundle::new_concurrent(vec![langid]);
    // Fluent wraps placeables in directionality marks, which a UI label does not want.
    bundle.set_use_isolating(false);
    bundle
        .add_resource(resource)
        .expect("a catalogue has no duplicate messages");

    // Anything needing arguments cannot be resolved once and kept, so it stays out of the map.
    let plain = KEYS
        .iter()
        .filter_map(|key| Some((*key, resolve(&bundle, key, None)?)))
        .collect();

    Loaded { bundle, plain }
}

fn resolve(
    bundle: &FluentBundle<FluentResource>,
    key: &str,
    args: Option<&Args<'_>>,
) -> Option<String> {
    let message = bundle.get_message(key)?;
    let pattern = message.value()?;
    let mut errors = Vec::new();
    let text = bundle.format_pattern(pattern, args, &mut errors);
    errors.is_empty().then(|| text.into_owned())
}

fn index_for(language: Language) -> usize {
    match language {
        Language::Auto => negotiate(sys_locale::get_locale().as_deref()),
        chosen => CATALOGUES
            .iter()
            .position(|catalogue| catalogue.language == chosen)
            .unwrap_or(0),
    }
}

// Exact tag first, then any catalogue sharing the language, then the source catalogue.
fn negotiate(requested: Option<&str>) -> usize {
    let Some(requested) = requested else {
        return 0;
    };
    let trimmed = requested.split('.').next().unwrap_or(requested);
    let Ok(wanted) = trimmed.replace('_', "-").parse::<LanguageIdentifier>() else {
        return 0;
    };

    let tag = |catalogue: &Catalogue| {
        catalogue
            .tag
            .parse::<LanguageIdentifier>()
            .expect("a catalogue tag parses")
    };
    CATALOGUES
        .iter()
        .position(|catalogue| tag(catalogue) == wanted)
        .or_else(|| {
            CATALOGUES
                .iter()
                .position(|catalogue| tag(catalogue).language == wanted.language)
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(source: &str) -> Vec<&str> {
        source
            .lines()
            .filter(|line| !line.starts_with([' ', '#', '*', '[', '.']))
            .filter_map(|line| line.split_once(" ="))
            .map(|(id, _)| id)
            .collect()
    }

    #[test]
    fn every_catalogue_defines_the_same_messages() {
        let source = ids(CATALOGUES[0].source);
        for catalogue in &CATALOGUES[1..] {
            assert_eq!(
                ids(catalogue.source),
                source,
                "{} has drifted from {}",
                catalogue.tag,
                CATALOGUES[0].tag
            );
        }
    }

    #[test]
    fn every_message_is_reachable_from_the_code() {
        let known: Vec<&str> = KEYS.to_vec();
        for id in ids(CATALOGUES[0].source) {
            assert!(known.contains(&id), "{id} is in the catalogue but unused");
        }
    }

    #[test]
    fn every_key_the_code_asks_for_is_translated() {
        for (index, catalogue) in CATALOGUES.iter().enumerate() {
            let loaded = build(index);
            for key in KEYS {
                assert!(
                    loaded.bundle.has_message(key),
                    "{} is missing {key}",
                    catalogue.tag
                );
            }
        }
    }

    #[test]
    fn a_locale_falls_back_to_the_language_then_to_the_source() {
        assert_eq!(negotiate(Some("en-US")), 1);
        assert_eq!(negotiate(Some("en_US.UTF-8")), 1);
        assert_eq!(negotiate(Some("en-GB")), 0);
        assert_eq!(negotiate(Some("en-AU")), 0, "another English");
        assert_eq!(negotiate(Some("ro-RO")), 2);
        assert_eq!(negotiate(Some("ro")), 2, "the language alone");
        assert_eq!(negotiate(Some("pt-PT")), 0, "no catalogue at all");
        assert_eq!(negotiate(Some("nonsense at all")), 0);
        assert_eq!(negotiate(None), 0);
    }

    #[test]
    fn a_chosen_language_beats_the_system_one() {
        assert_eq!(index_for(Language::EnGb), 0);
        assert_eq!(index_for(Language::EnUs), 1);
        assert_eq!(index_for(Language::Ro), 2);
    }

    #[test]
    fn the_two_englishes_differ_where_they_should() {
        let gb = build(0);
        let us = build(1);
        assert_eq!(gb.plain["colour"], "Colour");
        assert_eq!(us.plain["colour"], "Color");
        assert_eq!(gb.plain["undo"], us.plain["undo"]);
    }

    #[test]
    fn an_argument_message_is_resolved_on_demand_rather_than_cached() {
        let loaded = build(0);
        assert!(!loaded.plain.contains_key("about-version"));
        assert_eq!(
            resolve(
                &loaded.bundle,
                "about-version",
                Some(&Args::from_iter([("version", "0.2.2")]))
            ),
            Some("Version 0.2.2".to_owned())
        );
    }

    fn recovery_body(language: Language, count: usize) -> String {
        let loaded = build(index_for(language));
        resolve(
            &loaded.bundle,
            "recovery-body",
            Some(&Args::from_iter([("count", count)])),
        )
        .expect("recovery-body resolves")
    }

    #[test]
    fn a_count_picks_its_own_wording() {
        assert!(recovery_body(Language::EnGb, 1).starts_with("A document"));
        assert!(recovery_body(Language::EnGb, 3).starts_with("3 documents"));
    }

    #[test]
    fn romanian_reaches_for_its_third_plural_form() {
        let body = |count| recovery_body(Language::Ro, count);
        assert!(body(1).starts_with("Un document"), "{}", body(1));
        assert!(body(5).starts_with("5 documente"), "{}", body(5));
        assert!(body(19).starts_with("19 documente"), "{}", body(19));
        assert!(body(20).starts_with("20 de documente"), "{}", body(20));
        assert!(body(100).starts_with("100 de documente"), "{}", body(100));
    }

    #[test]
    fn a_long_note_stays_one_line_for_the_widget_to_wrap() {
        for index in 0..CATALOGUES.len() {
            for (key, text) in &build(index).plain {
                assert!(
                    !text.contains('\n'),
                    "{key} carries a line break the widget cannot rewrap"
                );
            }
        }
    }

    #[test]
    fn placeables_carry_no_directionality_marks() {
        let loaded = build(0);
        let name = resolve(
            &loaded.bundle,
            "window-title",
            Some(&Args::from_iter([("name", "Untitled")])),
        )
        .expect("window-title resolves");
        assert_eq!(name, "Untitled - RustyPaint");
    }
}
