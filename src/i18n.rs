//! What this application says, in the language the session speaks.
//!
//! The words themselves are in `locales/`, one Fluent catalog per language,
//! compiled into the binary; the machinery that reads them is the toolkit's,
//! so an application, the file question it raises and the shell it was opened
//! from all speak from catalogs of the same shape. See
//! `docs/localization.md`, and `lxb_app::lxb_toolkit::i18n` for how a language
//! is chosen.
//!
//! Anything a person, a device or a file gave its name to is passed as a
//! *value* and never as an identifier: a message id is this application's own
//! writing, and a file called `{ $name }.png` is still a file.
use std::sync::LazyLock;

pub use lxb_app::lxb_toolkit::i18n::{Catalog, FluentArgs};

/// Every language this application is translated into. A new one is a file
/// beside `en.ftl` and a line here; one that is missing a message falls back
/// to English on its own.
pub const RESOURCES: &[(&str, &str)] = &[
    ("en-GB", include_str!("../locales/en-GB.ftl")),
    ("de", include_str!("../locales/de.ftl")),
    ("es", include_str!("../locales/es.ftl")),
    ("fr", include_str!("../locales/fr.ftl")),
    ("hi", include_str!("../locales/hi.ftl")),
    ("pl", include_str!("../locales/pl.ftl")),
    ("pt-BR", include_str!("../locales/pt-BR.ftl")),
    ("ru", include_str!("../locales/ru.ftl")),
    ("zh-CN", include_str!("../locales/zh-CN.ftl")),
];

static CATALOG: LazyLock<Catalog> = LazyLock::new(|| Catalog::new(RESOURCES));

/// A label with nothing in it.
pub fn text(id: &'static str) -> &'static str {
    CATALOG.text(id)
}

/// A sentence with values in it. Reached through [`crate::message`].
pub fn format(id: &str, args: &FluentArgs<'_>) -> String {
    CATALOG.format(id, args)
}

/// A sentence with values in it.
///
/// A count is passed as a number — `"count" => files` — never as text: a
/// language picks the form of the noun by looking at the number, and Fluent
/// handed a string falls silently to the form nothing else uses.
#[macro_export]
macro_rules! message {
    ($id:literal $(, $name:literal => $value:expr)* $(,)?) => {{
        let mut args = $crate::i18n::FluentArgs::new();
        $(args.set($name, $value);)*
        $crate::i18n::format($id, &args)
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every language carries the same messages, with the same variables in
    /// them, and every one of them formats.
    #[test]
    fn the_catalogs_carry_the_same_messages() {
        Catalog::validate(RESOURCES);
    }

    /// And every message this application asks for is one they have — the
    /// failure that would otherwise be drawn on screen as the identifier
    /// itself, in every language including English.
    #[test]
    fn every_message_this_application_asks_for_is_one_the_catalogs_have() {
        Catalog::check_references(env!("CARGO_MANIFEST_DIR"), RESOURCES);
    }

    /// And every word of them can be drawn by a face the toolkit ships.
    ///
    /// Roboto has no Devanagari and no Han, so a Hindi or Chinese label would
    /// otherwise rasterise to a row of empty boxes, and nothing that reads
    /// strings rather than glyphs would say a word about it. The Han face is
    /// a subset of GB 2312, so this also says when a sentence has to be
    /// reworded rather than the face regrown.
    #[test]
    fn every_word_the_catalogs_carry_can_be_drawn() {
        lxb_app::lxb_render::faces::check_drawable(RESOURCES);
    }
}
