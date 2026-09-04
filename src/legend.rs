//! What the buttons do, drawn rather than spelled out.
//!
//! The shell writes the same row in the corner of its start screen, and this
//! is that row: a word and a picture of the button that does it, laid out from
//! the right-hand margin leftwards. It is drawn rather than lettered because
//! the same act is South on a pad and Enter on a keyboard, and there is no
//! wording that names both without naming neither.
//!
//! Two rules come with it, both the shell's:
//!
//! * **A legend naming a button that does nothing is worse than naming none.**
//!   Every hint here is asked of the same page that answers the press, so the
//!   two cannot drift.
//! * **Nothing about moving.** The arrows are the one thing a page does not
//!   have to explain, and a row that explained them would be four pairs long
//!   before it said anything.
//!
//! The row is also a row of controls. Every pair is a target a pointer can
//! land on, and a click on one is a press of the button it pictures — which is
//! how a mouse gets back out of a page whose Back is only ever drawn here. It
//! costs nothing: the legend is the last thing drawn on every page, so its own
//! targets are the last written down and win wherever they overlap.
//!
//! Which picture is drawn is `Page::pad_in_hand`, which the toolkit watches
//! the way the shell watches its own: a key or a click says a keyboard, an
//! action off a pad says a pad. Under the shell it starts from what the shell
//! last wrote down, and everywhere else from whether a pad is plugged in at
//! all — so the row is right on the first frame and stays right as the hands
//! move between the two.

use lxb_app::lxb_render::Align;
use lxb_app::lxb_toolkit::{input::Action, palette::Role, typography::Text};
use lxb_app::Page;

/// One thing a button does: the word, and which button it is on each of the
/// two things this might be being driven with.
#[derive(Debug, Clone, Copy)]
pub struct Hint {
    pub label: &'static str,
    /// The button on a pad, and the key on a keyboard.
    pub on: Button,
}

/// The buttons a legend ever names.
///
/// Three: the one that acts on what the light is on, the one that gets back
/// out, and the one that raises the other answers. Everything else on these
/// pages is an arrow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Accept,
    /// Where the *other* answers live — on these shelves, the order the
    /// listing is in. It is on the legend because without it there is nothing
    /// on the page to say a listing can be in any other order at all.
    Options,
    Back,
}

impl Button {
    /// The picture of this button, given what the user's hands are on.
    ///
    /// Options is the one place this legend leaves the keyboard, and it is the
    /// shell's own choice: a menu is raised with the right button by anybody
    /// holding a pointer, and no key printed on a keyboard says the same thing
    /// to as many people.
    /// What a press of this is, for a page that was pressed rather than
    /// pointed at. A click on a pair has to do exactly what the key it
    /// pictures does, or the row is a picture of something else.
    pub fn action(self) -> Action {
        match self {
            Button::Accept => Action::Accept,
            Button::Options => Action::Menu,
            Button::Back => Action::Back,
        }
    }

    pub fn glyph(self, pad: bool) -> &'static str {
        match (self, pad) {
            (Button::Accept, true) => "pad-south",
            (Button::Accept, false) => "key-enter",
            (Button::Options, true) => "pad-north",
            (Button::Options, false) => "mouse-right",
            (Button::Back, true) => "pad-east",
            (Button::Back, false) => "key-escape",
        }
    }
}

/// Shorthand for one pair.
pub const fn hint(label: &'static str, on: Button) -> Hint {
    Hint { label, on }
}

/// How large a legend is drawn.
///
/// The shell's own proportions, scaled: a picture of a button rather larger
/// than the word beside it, because the picture is the half being read.
const GLYPH: f32 = 28.0;
const GAP: f32 = 7.0;
const STEP: f32 = 22.0;

/// Where a pointer can land on this row. Kept clear of everything the pages
/// number, which is [`crate::draw`]'s business.
pub const SPOT: u32 = 0x8000;

/// Lay a legend out from `right` leftwards, centred on `middle`, and answer
/// where its left-hand end came out.
///
/// Right to left because the words are different lengths and the row is hung
/// off the margin: built that way, the pair nearest the corner lands exactly
/// on it whatever the words turn out to measure.
///
/// The left-hand end is worth having back. It is where anything else on that
/// line — a job's name, a note, a word about the machine being read again —
/// has to stop, and a page that guessed would write one over the other.
pub fn row(page: &mut Page, right: f32, middle: f32, hints: &[Hint], pad: bool) -> f32 {
    let glyph = page.scaled(GLYPH);
    let gap = page.scaled(GAP);
    let step = page.scaled(STEP);
    let label = page.line(Text::Caption);
    let icons = page.icons();

    let mut at = right;
    for (index, hint) in hints.iter().enumerate().rev() {
        let word = page.measure(Text::Caption, hint.label);
        let ends = at - glyph - gap;
        let ui = page.ui();
        // The word and the picture beside it are one target: they say the same
        // thing, so a click on either is a press of that button. Grown by half
        // a gap all round, which is comfortably inside the step between pairs
        // — no two of these may ever overlap, or a click would be answered by
        // whichever happened to be written down last.
        ui.spot(
            SPOT + index as u32,
            [
                ends - word - gap * 0.5,
                middle - glyph * 0.5 - gap * 0.5,
                word + gap * 2.0 + glyph,
                glyph + gap,
            ],
        );
        ui.icon_tinted(
            [at - glyph, middle - glyph * 0.5, glyph, glyph],
            hint.on.glyph(pad),
            icons,
            Role::Text,
            0.85,
        );
        ui.label(
            [ends - word, middle - label * 0.5, word, label],
            Text::Caption,
            hint.label,
            Role::TextSoft,
            Align::Right,
        );
        at = ends - word - step;
    }
    at
}

/// Which button a click on this row landed on, if it landed on it at all.
///
/// Asked of the same hints the row was drawn from, so a pair that was not
/// drawn cannot be pressed: a page that is showing a bar instead of a legend
/// wrote no targets down and answers nothing here.
pub fn pressed(page: &mut Page, hints: &[Hint]) -> Option<Button> {
    for (index, hint) in hints.iter().enumerate() {
        if page.pressed(SPOT + index as u32) {
            return Some(hint.on);
        }
    }
    None
}

/// How tall a row of this is, which is what the foot of a page keeps for it.
pub fn height(page: &Page) -> f32 {
    page.scaled(GLYPH).max(page.line(Text::Caption))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_button_is_pictured_by_a_mark_the_toolkit_has() {
        for button in [Button::Accept, Button::Options, Button::Back] {
            for pad in [true, false] {
                let glyph = button.glyph(pad);
                assert!(
                    lxb_app::lxb_toolkit::assets::glyph(glyph).is_some(),
                    "a legend asks for a mark that is not in the toolkit: {glyph}"
                );
            }
        }
    }
}
