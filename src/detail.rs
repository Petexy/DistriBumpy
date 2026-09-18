//! The pages that are about one thing: an application, a repository, or a
//! repository about to be added.
//!
//! An application's page is three bands — what can be done to it, what can be
//! read about it, and the reading itself. Up and Down cross between the bands
//! and Left and Right move within one, which is the only model that works on a
//! controller with four directions and no pointer, and is what the shell
//! itself does everywhere.
//!
//! Three bands, but not three lines. The controls and the tabs share one, and
//! the facts about the application stand beside its name rather than under it.
//! Both were stacked, and the page spent half its height on a name, nine short
//! chips and three words before the description and the screenshots began.
//!
//! The gallery is the part worth being careful about. Every screenshot's width
//! and height are declared in the catalogue, so its frame is cut to the shape
//! of the picture **before** the picture has been fetched: the frame never
//! changes shape underneath a picture landing in it, and a wide screenshot is
//! never shown letterboxed inside a tall box.

use lxb_app::lxb_render::{Align, Fit};
use lxb_app::lxb_toolkit::{material::Surface, metrics::Metric, palette::Role, typography::Text};
use lxb_app::Page;

use crate::catalogue::{Listing, Shot};
use crate::draw::{
    bar, beside_a_bar, one_line, shortened, BUTTON_SPOT, CONTENT_BAR_SPOT, CONTENT_SPOT,
    PICTURE_FADE, SHOT_SPOT, TAB_SPOT,
};
use crate::flatpak::{self, Scope};
use crate::sandbox::{Group, Standing};
use crate::store::{Band, Button, Store, Tab};

/// The shape a frame is cut to when the catalogue said nothing about the
/// picture, which is what nearly every screenshot on a desktop really is.
const ASSUMED: f32 = 16.0 / 9.0;

/// Below this width, measured in the design language's 1080-high reference
/// space, a detail page is a stack rather than a squeezed wide page.
const NARROW_DETAIL: f32 = 1420.0;

/// The share of the line the name column may take when the facts stand beside
/// it, and the least it is given.
///
/// The column takes what the name, the summary and the developer really
/// measure — which is why the facts get most of the line on nearly every
/// application. The cap is for the long names, whose column would otherwise
/// leave no room to lay a chip in; the floor is for the one-word names, whose
/// summary would otherwise wrap to four lines beside a nearly empty row.
const NAME_MOST: f32 = 0.44;
const NAME_LEAST: f32 = 190.0;

/// How far a button that is not the one in hand is washed back towards
/// `Role::Glass`. See `row_of_buttons`: what says which button a press would
/// reach is the step in depth between this and none of it.
pub(crate) const RESTING_WASH: f32 = 0.42;

#[derive(Debug, Clone)]
struct Fact {
    text: String,
    role: Role,
}

impl Fact {
    fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            role: Role::GlassRaised,
        }
    }

    fn accented(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            role: Role::Accent,
        }
    }

    fn warning(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            role: Role::Danger,
        }
    }
}

fn is_narrow(page: &Page, room: [f32; 4]) -> bool {
    room[2] < page.scaled(NARROW_DETAIL)
}

/// One application.
pub fn application(store: &mut Store, page: &mut Page, room: [f32; 4], id: &str) {
    let listing = store.catalogue.get(id).cloned();
    let installed = store.installed(id).cloned();
    let buttons = store.detail_buttons(id);
    let tabs = store.tabs(id);
    // The row of controls empties while a job is running elsewhere, and the
    // light may not be left standing in a band that is not there.
    store.settle_the_buttons(&buttons);
    let coming = store.anim.arriving();

    let name = listing
        .as_ref()
        .map(|one| one.name.clone())
        .or_else(|| installed.as_ref().map(|one| one.name.clone()))
        .unwrap_or_else(|| id.to_string());

    let gap = page.metric(Metric::Gap);

    let under = header(
        store,
        page,
        room,
        &name,
        listing.as_ref(),
        installed.as_ref(),
        coming,
    );

    // Every band's rectangles are worked out before anything is drawn, so that
    // the one light crossing the page can be laid down first — it is one
    // object and it belongs behind whatever it is under.
    //
    // **The controls and the tabs are one line.** Install, and then what there
    // is to read about the thing being installed, with the rule under the tabs
    // beginning where the controls stop. They had a band each, and the second
    // was three words with a whole band to itself — height the description and
    // the screenshots below wanted more than the tabs did.
    let places = button_places(page, room, under, &buttons);
    let after = places
        .iter()
        .map(|rect| rect[0] + rect[2])
        .fold(f32::MIN, f32::max);
    let after = if places.is_empty() {
        room[0]
    } else {
        after + gap * 1.4
    };
    // Only where the controls really came out on one line, and only where the
    // tabs fit in what is left of it: a page with four controls on a narrow
    // window puts them back under one another rather than cutting a tab in
    // half.
    let one_row = places.iter().all(|rect| rect[1] <= under + 0.5);
    let beside = one_row && after + tabs_width(page, &tabs) <= room[0] + room[2];
    let (tabs_left, tabs_top, tab_height) = if beside {
        // The same height as a control, so the two share a centre line and the
        // rule lands on the bottom of the capsule beside it.
        (after, under, page.metric(Metric::RowHeight))
    } else {
        (
            room[0],
            places_bottom(&places, under) + gap * 0.9,
            page.line(Text::Label) * 2.2,
        )
    };
    let tab_room = [
        tabs_left,
        room[1],
        (room[0] + room[2] - tabs_left).max(page.scaled(80.0)),
        room[3],
    ];
    let tab_places = tab_places(page, tab_room, tabs_top, &tabs, tab_height);
    let tabs_bottom = places_bottom(&tab_places, tabs_top);
    // Read back by moving: a direction has to mean what the page looks like it
    // means, and on one line Down through the controls would walk the light
    // sideways. See `Store::one_row_is`.
    store.one_row_is(beside);

    let content_top = tabs_bottom + gap;
    // The reading below, and the channel its bar stands in beside it. Kept
    // whether or not a bar is drawn, so nothing about the reading moves when
    // a hand leaves the pad — the listing's own half of this is `beside_a_bar`.
    let (content, track) = beside_a_bar(
        page,
        [
            room[0],
            content_top,
            room[2],
            (room[1] + room[3] - content_top).max(0.0),
        ],
    );

    let lit = match store.band {
        Band::Buttons => places.get(store.button).copied(),
        Band::Tabs => tab_places.get(store.tab).copied(),
        Band::Content => None,
    };
    if let Some(rect) = lit {
        page.glide(rect, coming);
    }

    row_of_buttons(
        store,
        page,
        &buttons,
        &places,
        store.band == Band::Buttons,
        coming,
    );
    row_of_tabs(store, page, &tabs, &tab_places, tabs_left, coming);

    match store.tab(id) {
        Tab::About => about(store, page, content, id, listing.as_ref(), coming),
        Tab::Changes => changes(store, page, content, listing.as_ref(), coming),
        Tab::Permissions => permissions(store, page, content, id, coming),
        Tab::Links => links(store, page, content, id, coming),
    }
    // After the band, which is what worked out how deep it runs.
    let laid = bar(page, track, store.content_bar(), CONTENT_BAR_SPOT);
    store.content_bar_is(laid.0, laid.1);
}

/// The icon, the name, who wrote it, and the handful of facts that decide
/// whether somebody presses Install — the facts standing beside the name
/// rather than under it. See `NAME_MOST` for how the line is divided.
#[allow(clippy::too_many_arguments)]
fn header(
    store: &mut Store,
    page: &mut Page,
    room: [f32; 4],
    name: &str,
    listing: Option<&Listing>,
    installed: Option<&flatpak::Installed>,
    coming: f32,
) -> f32 {
    let narrow = is_narrow(page, room);
    let mark = page.scaled(if narrow { 78.0 } else { 104.0 });
    let gap = page.metric(Metric::Gap);
    let title_text = if narrow { Text::Title } else { Text::Display };
    let title_line = page.line(title_text);
    let body_line = page.line(Text::Body);
    let caption = page.line(Text::Caption);

    let icon_at = [room[0], room[1], mark, mark];
    let text_left = room[0] + mark + gap * if narrow { 1.1 } else { 1.6 };
    let verified = listing.is_some_and(|one| one.verified);
    let text_room = (room[0] + room[2] - text_left).max(page.scaled(80.0));

    let developer = listing
        .map(|one| one.developer.clone())
        .filter(|said| !said.is_empty())
        .map(|said| crate::message!("by-publisher", "publisher" => (said).to_string()));
    let summary = listing
        .map(|one| one.summary.clone())
        .or_else(|| installed.map(|one| one.name.clone()))
        .unwrap_or_default();
    let mut facts = facts_of(store, listing, installed);
    if verified {
        facts.insert(0, Fact::accented(crate::i18n::text("verified-publisher")));
    }
    let eol = installed.and_then(|one| one.eol.clone());

    // **The facts stand beside the name.** They are short and there are up to
    // nine of them, and a block of them across the whole line wrapped to three
    // rows of chips with a hole at the end of the last — a band of the page
    // spent on small print, with the description and the screenshots pushed
    // down under it. Beside the name the two columns come out much the same
    // height, and the reading below begins where the icon ends.
    //
    // Not on a narrow window, where there is no line to divide, and not where
    // there are no facts, where the name may as well have the whole of it.
    let beside = !narrow && !facts.is_empty();
    let text_width = if beside {
        // What the name column really measures, before anything is cut. A
        // short name and a short summary leave the facts nearly the whole
        // line, which is the shape this page was drawn to.
        let wanted = [
            page.measure(title_text, name),
            page.measure(Text::Body, &summary),
            developer
                .as_deref()
                .map(|said| page.measure(Text::Caption, said))
                .unwrap_or(0.0),
        ]
        .into_iter()
        .fold(0.0f32, f32::max);
        let most = text_room * NAME_MOST;
        wanted.clamp(page.scaled(NAME_LEAST).min(most), most)
    } else {
        text_room
    };

    let title = one_line(page, title_text, name, text_width);
    let mut summary_lines = if summary.is_empty() {
        Vec::new()
    } else {
        page.ui().wrap(Text::Body, &summary, text_width)
    };
    // Two lines wherever the column is only as wide as the name needs: a
    // summary is a sentence, and one line of it in a narrow column is a
    // sentence cut off after four words.
    let summary_room = if narrow || beside { 2 } else { 1 };
    if summary_lines.len() > summary_room {
        summary_lines.truncate(summary_room);
        if let Some(last) = summary_lines.last_mut() {
            *last = shortened(page, Text::Body, last, text_width);
        }
    }

    let fetched = listing
        .filter(|one| one.icon.is_none())
        .and_then(|one| one.icon_remote.clone())
        .and_then(|url| {
            let fade = store.art.fade(&url, PICTURE_FADE);
            store.art.picture(&url).map(|path| (path, fade))
        });

    {
        let icons = page.icons();
        let ui = page.ui();
        let radius = ui.m(Metric::CardRadius);
        let drawn = match (listing.and_then(|one| one.icon.as_ref()), &fetched) {
            (Some(path), _) => ui.picture(icon_at, radius, path, Fit::Contain, coming),
            (None, Some((path, fade))) => {
                ui.picture(icon_at, radius, path, Fit::Contain, coming * fade)
            }
            (None, None) => false,
        };
        if !drawn {
            ui.icon_tinted(icon_at, "launch", icons, Role::AccentSoft, 0.7 * coming);
        }
    }

    let mut top = room[1];
    {
        let ui = page.ui();
        ui.label_tinted(
            [text_left, top, text_width, title_line],
            title_text,
            &title,
            ui.tinted(Role::Text, coming),
            Align::Left,
        );
    }
    top += title_line;

    for line in &summary_lines {
        let ui = page.ui();
        ui.label_tinted(
            [text_left, top, text_width, body_line],
            Text::Body,
            line,
            ui.tinted(Role::Text, coming * 0.94),
            Align::Left,
        );
        top += body_line;
    }
    if let Some(developer) = developer {
        let developer = one_line(page, Text::Caption, &developer, text_width);
        let ui = page.ui();
        ui.label_tinted(
            [text_left, top, text_width, caption],
            Text::Caption,
            &developer,
            ui.tinted(Role::TextSoft, coming),
            Align::Left,
        );
        top += caption;
    }

    let (detail_left, detail_width) = if narrow {
        (room[0], room[2])
    } else {
        (text_left, text_room)
    };
    let mut bottom = if beside {
        // Hung so that the first row of chips sits on the name's own line,
        // rather than on the top of the box that line is set in.
        let chips_top = room[1] + ((title_line - chip_height(page)) * 0.5).max(0.0);
        let facts_left = text_left + text_width + gap * 1.6;
        let facts_width = (room[0] + room[2] - facts_left).max(0.0);
        top.max(metadata_chips(
            page,
            [facts_left, chips_top, facts_width],
            &facts,
            coming,
        ))
    } else {
        if narrow {
            top = top.max(room[1] + mark) + gap * 0.6;
        } else {
            top += gap * 0.35;
        }
        metadata_chips(page, [detail_left, top, detail_width], &facts, coming)
    };

    // Under both columns whichever way they came out: this is a warning about
    // the application, not one more fact about it.
    if let Some(said) = eol {
        let warning = crate::message!("no-longer-updated-by", "publisher" => (said).to_string());
        let mut lines = page.ui().wrap(Text::Caption, &warning, detail_width);
        let cut = lines.len() > 3;
        lines.truncate(3);
        if cut {
            if let Some(last) = lines.last_mut() {
                *last = shortened(page, Text::Caption, last, detail_width);
            }
        }
        bottom += gap * 0.35;
        for line in lines {
            let ui = page.ui();
            ui.label_tinted(
                [detail_left, bottom, detail_width, caption],
                Text::Caption,
                &line,
                ui.tinted(Role::Danger, coming),
                Align::Left,
            );
            bottom += caption;
        }
    }

    room[1] + mark.max(bottom - room[1]) + gap * if narrow { 0.8 } else { 1.1 }
}

/// Push each row of a laid-out block over so that it ends on `right`.
///
/// A row at a time, not the block as a whole: what is wanted is a straight
/// right edge, and shifting every row by one amount would keep the ragged edge
/// and merely move it. Rows are told apart by their tops, which chips laid on
/// the same line share exactly.
fn hang_right(rects: &mut [[f32; 4]], right: f32) {
    let mut from = 0;
    while from < rects.len() {
        let top = rects[from][1];
        let mut until = from + 1;
        while until < rects.len() && rects[until][1] == top {
            until += 1;
        }
        let ends = rects[until - 1];
        let over = right - (ends[0] + ends[2]);
        for rect in &mut rects[from..until] {
            rect[0] += over;
        }
        from = until;
    }
}

/// How many rows a wrap that fills each row before starting the next takes,
/// given nothing may be wider than `limit`.
///
/// A chip too wide for the limit still goes on a row of its own rather than
/// nowhere, which is what stops this running out of rows to try.
fn rows_at(widths: &[f32], gap: f32, limit: f32) -> usize {
    let mut rows = 1;
    let mut used = 0.0;
    for &width in widths {
        if used > 0.0 && used + gap + width > limit {
            rows += 1;
            used = width;
        } else if used > 0.0 {
            used += gap + width;
        } else {
            used = width;
        }
    }
    rows
}

/// The narrowest the block can be drawn and still cost no more rows than it
/// has to.
///
/// Filling each row before starting the next leaves whatever is over alone on
/// the last one — one short chip under a full line, which reads as a mistake
/// rather than as a wrap, and spends a whole row on a single word. The same
/// chips laid out inside the narrowest width that costs nothing spread evenly
/// instead. Bisection rather than arithmetic: the widths are a handful, and
/// the answer is whichever run of them happens to be longest.
fn balanced_limit(widths: &[f32], gap: f32, room: f32) -> f32 {
    let least = rows_at(widths, gap, room);
    let mut narrow = widths.iter().copied().fold(0.0f32, f32::max);
    let mut wide = room;
    while wide - narrow > 0.5 {
        let between = 0.5 * (narrow + wide);
        if rows_at(widths, gap, between) <= least {
            wide = between;
        } else {
            narrow = between;
        }
    }
    wide
}

/// How tall one chip is.
///
/// The header wants it as well as the block itself: a column of facts beside a
/// name is hung so that its first row sits on the name's own line, and it
/// cannot work that out without knowing how tall a row of it is.
fn chip_height(page: &mut Page) -> f32 {
    page.line(Text::Caption) * 1.65
}

/// Draw metadata as short, labelled pieces that wrap as whole chips. A detail
/// page used to flatten all of this into one long sentence; at narrow widths
/// its most important facts were simply the first ones to leave the window.
///
/// **Hung off the right-hand end of its room, a row at a time.** The block
/// stands beside the name and reaches the page's margin, so the margin is the
/// edge it shares with everything below it — the rule under the tabs, the
/// gallery, the date on a release. Laid from the left it ended on a different
/// ragged edge on every application, and a wrapped second row hung in the
/// middle of the line with the margin empty beside it.
fn metadata_chips(page: &mut Page, at: [f32; 3], facts: &[Fact], coming: f32) -> f32 {
    if facts.is_empty() || at[2] <= 0.0 {
        return at[1];
    }

    let pad = page.metric(Metric::RowPadding) * 0.62;
    let gap = page.metric(Metric::Gap) * 0.55;
    let height = chip_height(page);
    // Leave a small measurement cushion. Font shaping can land a fraction
    // wider than the advance used for planning; without it short facts at the
    // end of a row gained an ellipsis despite visibly having room. A hair of
    // it, not a share of the gap: every chip carried that share, so the more
    // facts an application had the sooner they wrapped.
    let slack = page.scaled(1.5);
    let widths: Vec<f32> = facts
        .iter()
        .map(|fact| (page.measure(Text::Caption, &fact.text) + pad * 2.0 + slack).min(at[2]))
        .collect();
    let limit = balanced_limit(&widths, gap, at[2]);

    let mut left = at[0];
    let mut top = at[1];
    let mut planned = Vec::new();

    // Where each row of chips begins and ends is settled first, laying them
    // from the left as they wrap; then every row is pushed over so that it
    // ends on the right-hand edge of the room.
    for (fact, width) in facts.iter().zip(&widths) {
        let width = *width;
        if left > at[0] && left - at[0] + width > limit {
            left = at[0];
            top += height + gap;
        }
        let shown = one_line(
            page,
            Text::Caption,
            &fact.text,
            (width - pad * 2.0).max(1.0),
        );
        planned.push((fact.role, shown, [left, top, width, height]));
        left += width + gap;
    }
    let mut laid: Vec<[f32; 4]> = planned.iter().map(|(_, _, rect)| *rect).collect();
    hang_right(&mut laid, at[0] + at[2]);
    for ((_, _, rect), moved) in planned.iter_mut().zip(laid) {
        *rect = moved;
    }

    for (role, text, rect) in &planned {
        let strength = if *role == Role::GlassRaised {
            0.16
        } else {
            0.28
        };
        let ui = page.ui();
        ui.chip(*rect, ui.tinted(*role, strength * coming));
        ui.label_tinted(
            *rect,
            Text::Caption,
            text,
            ui.tinted(Role::Text, coming * 0.94),
            Align::Centre,
        );
    }

    planned
        .last()
        .map(|(_, _, rect)| rect[1] + rect[3])
        .unwrap_or(at[1])
}

fn button_places(page: &mut Page, room: [f32; 4], top: f32, buttons: &[Button]) -> Vec<[f32; 4]> {
    let gap = page.metric(Metric::Gap);
    let height = page.metric(Metric::RowHeight);
    let pad = page.metric(Metric::RowPadding);
    let mut left = room[0];
    let mut row_top = top;
    let right = room[0] + room[2];
    let mut places = Vec::new();
    for button in buttons {
        let width = (page.measure(Text::Label, button.label()) + pad * 3.0).min(room[2]);
        if left > room[0] && left + width > right {
            left = room[0];
            row_top += height + gap * 0.65;
        }
        places.push([left, row_top, width, height]);
        left += width + gap * 0.8;
    }
    places
}

fn places_bottom(places: &[[f32; 4]], fallback: f32) -> f32 {
    places
        .iter()
        .map(|rect| rect[1] + rect[3])
        .fold(fallback, f32::max)
}

fn row_of_buttons(
    store: &mut Store,
    page: &mut Page,
    buttons: &[Button],
    places: &[[f32; 4]],
    band: bool,
    coming: f32,
) {
    // Only one ordinary action is primary. Update wins over Open when both are
    // present because it is the unfinished job on the page; otherwise the
    // first install/open/repository-state action is the obvious next step.
    let primary = buttons.iter().position(|button| {
        matches!(
            button,
            Button::Install | Button::Update | Button::Open | Button::RepoOn | Button::RepoOff
        )
    });
    for (index, (button, rect)) in buttons.iter().zip(places.iter()).enumerate() {
        let lit = band && index == store.button;
        let press = store.anim.press(lit);
        let ui = page.ui();
        ui.spot(BUTTON_SPOT + index as u32, *rect);
        let destructive = button.grave() || *button == Button::Stop;
        let is_primary = primary == Some(index) && !destructive;
        // The glow belongs to the light, not to the call to action. It used to
        // sit under whichever button was primary, which meant the two accented
        // shapes in a row were "the obvious next step" and "where you are" —
        // the same colour saying two different things.
        if lit {
            ui.glow(
                lxb_app::lxb_toolkit::motion::scaled_about_centre(*rect, 1.13),
                Role::Accent,
                0.26 * coming,
            );
        } else if destructive {
            let sunk = lxb_app::lxb_toolkit::motion::pressed(*rect, press.through());
            ui.glow(
                lxb_app::lxb_toolkit::motion::scaled_about_centre(sunk, 1.16),
                Role::Danger,
                0.25 * coming,
            );
        }

        let radius = rect[3] * 0.5;
        let sunk = ui.control(*rect, radius, press, rect[2], coming);
        // **A button that is not the one in hand steps back into the page.**
        // `Ui::control` lays a pale `GlassRaised` capsule, and the light over
        // it is `Role::Accent` — which on this page is a purple capsule lit by
        // a purple. The two came out a few points apart and nothing said which
        // button a press would reach. The same trap as a mark drawn in the
        // accent, and the same answer: the difference cannot be more of the
        // accent, so it is depth instead. A resting control is washed towards
        // `Role::Glass`, which is nearly black, and the lit one is left as the
        // language cuts it. Bright against recessed reads at a glance where
        // purple against purple did not.
        if !lit {
            ui.chip(sunk, ui.tinted(Role::Glass, RESTING_WASH * coming));
        }
        // Put semantic colour over the neutral glass, not underneath it: the
        // control surface is intentionally substantial and otherwise hides a
        // destructive wash almost completely. The primary's accent is quiet
        // and only while the light is elsewhere: under the light it would be
        // the same colour twice over, and out of it a call to action still has
        // to be visible.
        if is_primary && !lit {
            ui.chip(sunk, ui.tinted(Role::Accent, 0.22 * coming));
        } else if destructive {
            ui.chip(sunk, ui.tinted(Role::Danger, 0.18 * coming));
        }
        let ink = if lit { 1.0 } else { 0.88 };
        ui.label_weighted(
            sunk,
            Text::Label,
            button.label(),
            ui.tinted(Role::Text, coming * ink),
            Align::Centre,
            is_primary || lit,
        );
    }
}

/// How wide a row of tabs comes out laid end to end, which is what decides
/// whether it goes beside the controls or under them.
fn tabs_width(page: &mut Page, tabs: &[Tab]) -> f32 {
    let gap = page.metric(Metric::Gap);
    let pad = page.metric(Metric::RowPadding);
    let mut width = 0.0;
    for (index, tab) in tabs.iter().enumerate() {
        if index > 0 {
            width += gap * 0.6;
        }
        width += page.measure(Text::Label, tab.title()) + pad * 2.0;
    }
    width
}

fn tab_places(
    page: &mut Page,
    room: [f32; 4],
    top: f32,
    tabs: &[Tab],
    height: f32,
) -> Vec<[f32; 4]> {
    let gap = page.metric(Metric::Gap);
    let pad = page.metric(Metric::RowPadding);
    let mut left = room[0];
    let mut row_top = top;
    let right = room[0] + room[2];
    let mut places = Vec::new();
    for tab in tabs {
        let width = (page.measure(Text::Label, tab.title()) + pad * 2.0).min(room[2]);
        if left > room[0] && left + width > right {
            left = room[0];
            row_top += height + gap * 0.35;
        }
        places.push([left, row_top, width, height]);
        left += width + gap * 0.6;
    }
    places
}

fn row_of_tabs(
    store: &mut Store,
    page: &mut Page,
    tabs: &[Tab],
    places: &[[f32; 4]],
    from: f32,
    coming: f32,
) {
    let room = page.cursor();
    let right = room[0] + room[2];
    let thick = page.scaled(2.0).max(1.0);
    let chosen = store.tab.min(tabs.len().saturating_sub(1));
    let bottom = places_bottom(places, room[1]);
    let ui = page.ui();

    // The rule the row sits on, and the mark under whichever tab is showing.
    // The mark is a rule rather than a moving pill because a pill would be a
    // second light on a page that already has one.
    //
    // It begins where the tabs do rather than at the margin: run under the
    // controls beside them it would read as a line struck through the row, and
    // it is the tabs it belongs to.
    //
    // **A chip rather than `Ui::rule`, so that it can be faded.** `Ui::rule`
    // takes a role and lays it at the role's own strength, with nowhere to put
    // `coming` — so a page crossing back into its card took everything else
    // down with it and left this line lying across the listing at full white
    // until the last frame. A chip is `Quad::solid` with a capsule corner, and
    // a corner of half of two points is the same straight line. The mark under
    // the chosen tab, four lines below, was a chip for the same reason.
    ui.chip(
        [from, bottom - thick, (right - from).max(0.0), thick],
        ui.tinted(Role::Rim, coming),
    );
    for (index, (tab, rect)) in tabs.iter().zip(places.iter()).enumerate() {
        let showing = index == chosen;
        ui.spot(TAB_SPOT + index as u32, *rect);
        let ink = if showing { Role::Text } else { Role::TextSoft };
        ui.label_tinted(
            *rect,
            Text::Label,
            tab.title(),
            ui.tinted(ink, coming),
            Align::Centre,
        );
        if showing {
            ui.chip(
                [
                    rect[0] + rect[2] * 0.12,
                    rect[1] + rect[3] - thick * 1.5,
                    rect[2] * 0.76,
                    thick * 1.5,
                ],
                ui.tinted(Role::Accent, coming),
            );
        }
    }
}

/// The description, and the gallery beside it.
fn about(
    store: &mut Store,
    page: &mut Page,
    room: [f32; 4],
    id: &str,
    listing: Option<&Listing>,
    coming: f32,
) {
    let gap = page.metric(Metric::Gap);
    let shots: Vec<Shot> = listing
        .map(|one| one.screenshots.clone())
        .unwrap_or_default();
    let stacked = !shots.is_empty() && is_narrow(page, room);
    let (prose, gallery_at) = if shots.is_empty() {
        (room, None)
    } else if stacked {
        // A narrow page gives the screenshot the first half of the content and
        // the description the full width beneath it. Keeping the old two
        // columns here made both halves too narrow to be useful.
        let gallery_height = room[3] * 0.56;
        let below = room[1] + gallery_height + gap;
        (
            [
                room[0],
                below,
                room[2],
                (room[1] + room[3] - below).max(0.0),
            ],
            Some([room[0], room[1], room[2], gallery_height]),
        )
    } else {
        let split = (room[2] * 0.46).max(page.scaled(200.0));
        (
            [room[0], room[1], split, room[3]],
            Some([
                room[0] + split + gap * 1.6,
                room[1],
                (room[2] - split - gap * 1.6).max(page.scaled(120.0)),
                room[3],
            ]),
        )
    };

    let description = listing
        .map(|one| {
            if one.description.is_empty() {
                one.summary.clone()
            } else {
                one.description.clone()
            }
        })
        .unwrap_or_default();

    // How many lines there are, and how many fit: a description is scrolled by
    // line rather than clipped, because the whole of it is worth reading and
    // there is nowhere else on this page to put it.
    let line = page.line(Text::Body);
    let fits = (prose[3] / line).floor().max(1.0) as usize;
    let lines = page.ui().wrap(Text::Body, &description, prose[2]);
    let deep = lines.len().saturating_sub(fits) + 1;
    store.content_is(deep, fits);

    let from = store.content.min(deep.saturating_sub(1));
    let band = store.band == Band::Content;
    let radius = page.metric(Metric::CardRadius);
    let ui = page.ui();
    // Prose is not a row, so there is no light to sit on it. What says the
    // reading is in hand is a quiet outline round the column — the same one a
    // control wears when it is out of its resting state.
    if band {
        ui.control_out(
            [
                prose[0] - gap * 0.5,
                prose[1] - gap * 0.4,
                prose[2] + gap,
                prose[3] + gap * 0.8,
            ],
            radius,
            coming,
        );
    }
    for (offset, text) in lines.iter().skip(from).take(fits).enumerate() {
        ui.label_tinted(
            [prose[0], prose[1] + line * offset as f32, prose[2], line],
            Text::Body,
            text,
            ui.tinted(Role::Text, coming * 0.92),
            Align::Left,
        );
    }
    if lines.len() > fits {
        // A hairline down the edge of the prose, saying how much of it is on
        // the screen and where. It is not the bar: the bar beside this page's
        // content is the whole band's and this is one column of it, so what
        // stands here says how far down the reading has got and is not
        // something to take hold of.
        let rail = ui.s(3.0).max(2.0);
        let run = (fits as f32 / lines.len() as f32).clamp(0.1, 1.0);
        let at = if deep > 1 {
            from as f32 / (deep - 1) as f32
        } else {
            0.0
        };
        let track = [prose[0] - gap * 0.8, prose[1], rail, prose[3]];
        ui.chip(track, ui.tinted(Role::Glass, 0.5 * coming));
        ui.chip(
            [
                track[0],
                track[1] + (track[3] - track[3] * run) * at,
                rail,
                track[3] * run,
            ],
            ui.tinted(if band { Role::Accent } else { Role::TextSoft }, coming),
        );
    }

    if let Some(gallery_at) = gallery_at {
        gallery(store, page, gallery_at, id, &shots, coming);
    }
}

/// One screenshot in a frame cut to its shape, with the rest of them under it.
fn gallery(
    store: &mut Store,
    page: &mut Page,
    room: [f32; 4],
    _id: &str,
    shots: &[Shot],
    coming: f32,
) {
    let gap = page.metric(Metric::Gap);
    let caption = page.line(Text::Caption);
    let at = store.shot.min(shots.len() - 1);
    let shown = &shots[at];

    // The strip of the others, sized first so that the hero gets what is left.
    let strip_height = if shots.len() > 1 {
        page.scaled(54.0)
    } else {
        0.0
    };
    let strip_room = if strip_height > 0.0 {
        strip_height + gap
    } else {
        0.0
    };
    let caption_room = if shown.caption.is_empty() {
        0.0
    } else {
        caption * 1.6
    };

    let box_at = [
        room[0],
        room[1],
        room[2],
        (room[3] - strip_room - caption_room).max(page.scaled(60.0)),
    ];
    // The frame is the picture's own shape, worked out from what the catalogue
    // declared rather than from the file — which is what lets it be right
    // before the file is here.
    let frame = crate::motion::framed(box_at, shown.aspect().unwrap_or(ASSUMED));

    let waiting = store.art.waiting(&shown.url);
    let fade = store.art.fade(&shown.url, PICTURE_FADE);
    let picture = store.art.picture(&shown.url);
    // The one either side, asked for now so that stepping across shows a
    // picture rather than the words under where a picture will be.
    for offset in [1, shots.len().saturating_sub(1)] {
        if let Some(next) = shots.get((at + offset) % shots.len()) {
            store.art.ask_for(&next.url);
        }
    }

    // Cut to the picture it sits under.
    //
    // A caption is whatever the publisher wrote in the AppStream entry, which
    // is alt text and is often a whole paragraph describing the picture for
    // somebody who cannot see it. `label_tinted` draws one centred line and
    // clips nothing, so a long one ran off both edges of the window and out of
    // the page entirely.
    let caption_said = (!shown.caption.is_empty())
        .then(|| one_line(page, Text::Caption, &shown.caption, frame[2]));

    let crossing = store.anim.crossing;
    let radius = page.metric(Metric::CardRadius);
    plate(page, frame, coming);
    let ui = page.ui();
    let drawn = picture.as_ref().is_some_and(|path| {
        // The frame is the picture's shape, so covering it fills it exactly:
        // nothing is cropped and there is no hairline of card down the side.
        ui.picture(frame, radius, path, Fit::Cover, coming * fade * crossing)
    });
    if !drawn {
        ui.label_tinted(
            frame,
            Text::Caption,
            if waiting {
                crate::i18n::text("fetching-a-picture")
            } else {
                crate::i18n::text("no-picture")
            },
            ui.tinted(Role::TextSoft, coming),
            Align::Centre,
        );
    }
    if let Some(said) = &caption_said {
        ui.label_tinted(
            [frame[0], box_at[1] + box_at[3], frame[2], caption_room],
            Text::Caption,
            said,
            ui.tinted(Role::TextSoft, coming * crossing),
            Align::Centre,
        );
    }

    if strip_height <= 0.0 {
        return;
    }
    let cell = strip_height * ASSUMED;
    let step = cell + gap * 0.5;
    // Lined up under the picture rather than under the box the picture sits
    // in: the frame is cut to the picture's shape, so the two are not the
    // same edge and a strip on the box's edge would sit off to one side.
    let fits = ((frame[2] + gap * 0.5) / step).floor().max(1.0) as usize;
    let from = at
        .saturating_sub(fits / 2)
        .min(shots.len().saturating_sub(fits.min(shots.len())));
    let top = room[1] + room[3] - strip_height;

    for (offset, shot) in shots.iter().enumerate().skip(from).take(fits) {
        let rect = [
            frame[0] + step * (offset - from) as f32,
            top,
            cell,
            strip_height,
        ];
        let here = offset == at;
        let store_fade = store.art.fade(&shot.url, PICTURE_FADE);
        let path = store.art.picture(&shot.url);
        plate(page, rect, coming);
        let ui = page.ui();
        ui.spot(SHOT_SPOT + 1 + offset as u32, rect);
        let alpha = if here { 1.0 } else { 0.55 };
        if let Some(path) = &path {
            // Cropped rather than shrunk: a thumbnail wants to be the same
            // shape as every other thumbnail beside it.
            //
            // The card's own radius, and not a fraction of it. `Ui::card` is
            // drawn at `Metric::CardRadius` whatever it is given, so a picture
            // rounded any less than that stands outside the plate it is on at
            // all four corners — and a screenshot whose own edges are bright
            // shows it plainly. Firefox's are: they are a 16:9 canvas with the
            // window opaque to 99% of the height, so the corners were white
            // and there was no plate under them.
            ui.picture(rect, radius, path, Fit::Cover, coming * store_fade * alpha);
        }
        if here {
            ui.control_out(rect, radius, coming);
        }
    }
    if shots.len() > fits {
        let said = crate::message!("place-of-total", "place" => at + 1, "total" => shots.len());
        let ui = page.ui();
        let ink = ui.tinted(Role::TextSoft, coming);
        ui.label_tinted(
            [frame[0], top - caption, frame[2], caption],
            Text::Caption,
            &said,
            ink,
            Align::Right,
        );
    }
}

/// Carry the light to a row and draw it in that row's own shape.
///
/// `Page::glide` draws a **capsule** whatever it is under, which is right for a
/// row one line high and wrong for one four lines high: half of a hundred-point
/// row is a fifty-point curve, and the first and last lines of the row run
/// straight out through it. So the spring is driven with the light turned off —
/// The plate a screenshot stands on, faded with the page it is on.
///
/// **`Ui::card`'s alpha is a stain, not an opacity**: it says how strongly the
/// role tints the surface, and a glass quad's material is not scaled by its own
/// tint. A plate asked for at four tenths of nothing therefore refracts,
/// glosses and rims exactly as hard as one at rest, and then goes out in a
/// single frame. That is what a page crossing back into its card left lying
/// over the listing after everything that could fade had gone — reported as a
/// shadow of the button. `Ui::fade_between` is the one channel the shader
/// carries through every kind of quad, so the plate now goes out with the
/// picture standing on it.
fn plate(page: &mut Page, rect: [f32; 4], coming: f32) {
    let ui = page.ui();
    let from = ui.written();
    ui.card(rect, Surface::Control, Role::Glass, PLATE_STAIN);
    let to = ui.written();
    ui.fade_between(from, to, coming);
}

/// How strongly a plate is tinted towards `Role::Glass` — a stain, and nothing
/// to do with how far the page it is on has arrived. See [`plate`].
const PLATE_STAIN: f32 = 0.4;

/// `Ui::lit` draws nothing at no strength, and still hands back where the light
/// has got to — and it is laid again at a radius that belongs to the row.
///
/// The radius is capped at half the shorter side, which is the capsule again:
/// a squircle asked for more than that is not a shape.
fn light_on(page: &mut Page, rect: [f32; 4], radius: f32, strength: f32) {
    let at = page.glide(rect, 0.0);
    let radius = radius.min(at[3] * 0.5).min(at[2] * 0.5);
    page.ui().lit(
        at,
        radius,
        lxb_app::lxb_toolkit::control::LIT_ROLE,
        at[2],
        strength,
    );
}

/// What changed, newest first.
fn changes(
    store: &mut Store,
    page: &mut Page,
    room: [f32; 4],
    listing: Option<&Listing>,
    coming: f32,
) {
    let releases = listing.map(|one| one.releases.clone()).unwrap_or_default();
    let gap = page.metric(Metric::Gap);
    let title = page.line(Text::Body);
    let caption = page.line(Text::Caption);
    let radius = page.metric(Metric::CardRadius);
    // How far the words are set in from the row's own edges. A row of one line
    // can be a capsule and put its words on its left edge, because at half its
    // height the curve has come and gone. A row of four cannot: see `light_on`.
    let pad = page.metric(Metric::RowPadding) * 0.75;
    let width = ((room[2] - pad * 2.0) * 0.72).max(page.scaled(240.0));

    // Every release is as tall as what it says, so the heights are worked out
    // before anything is drawn and the light is laid on the one it is on.
    let least = page.metric(Metric::RowHeight) * 0.85;
    let mut heights = Vec::new();
    for release in &releases {
        let lines = if release.notes.is_empty() {
            0
        } else {
            page.ui()
                .wrap(Text::Caption, &release.notes, width)
                .len()
                .min(6)
        };
        // A release nobody wrote a note for is a row of one line, and a row of
        // one line still has to be a row the light can sit on without hugging
        // the words inside it.
        heights.push((title + caption * lines as f32 + gap * 1.2).max(least));
    }

    let fits = fitting(&heights, store.content_top, room[3]);
    store.content_is(releases.len().max(1), fits.max(1));

    let from = store.content_top.min(releases.len().saturating_sub(1));
    let band = store.band == Band::Content;
    let mut top = room[1];
    for (index, release) in releases.iter().enumerate().skip(from) {
        let height = heights[index];
        if top + height > room[1] + room[3] {
            break;
        }
        let rect = [room[0], top, room[2], height];
        if band && index == store.content {
            light_on(page, rect, radius, coming);
        }
        // Inside the shape, not on its edge: a row this tall is a card rather
        // than a capsule, and a card's words are set in from its corners.
        let said = rect[0] + pad;
        let across = rect[2] - pad * 2.0;
        let lines = if release.notes.is_empty() {
            Vec::new()
        } else {
            page.ui()
                .wrap(Text::Caption, &release.notes, width)
                .into_iter()
                .take(6)
                .collect()
        };
        // Centred where there is nothing under it, and at the top of the row
        // where there is.
        let at = if lines.is_empty() {
            rect[1] + (rect[3] - title) * 0.5
        } else {
            rect[1] + gap * 0.6
        };
        let ui = page.ui();
        ui.spot(CONTENT_SPOT + index as u32, rect);
        ui.label_tinted(
            [said, at, width, title],
            Text::Body,
            &if release.version.is_empty() {
                crate::i18n::text("a-release").to_string()
            } else {
                crate::message!("version-number", "version" => release.version.to_string())
            },
            ui.tinted(Role::Text, coming),
            Align::Left,
        );
        if !release.when.is_empty() {
            ui.label_tinted(
                [said, at, across, title],
                Text::Caption,
                &release.when,
                ui.tinted(Role::TextSoft, coming * 0.8),
                Align::Right,
            );
        }
        for (offset, text) in lines.iter().enumerate() {
            ui.label_tinted(
                [said, at + title + caption * offset as f32, width, caption],
                Text::Caption,
                text,
                ui.tinted(Role::TextSoft, coming * 0.9),
                Align::Left,
            );
        }
        top += height;
    }
}

/// How many rows of assorted heights fit, starting from one of them.
fn fitting(heights: &[f32], from: usize, room: f32) -> usize {
    let mut used = 0.0;
    let mut count = 0;
    for height in heights.iter().skip(from) {
        if used + height > room {
            break;
        }
        used += height;
        count += 1;
    }
    count.max(1)
}

/// What an application may reach outside its sandbox, and the switches for it.
fn permissions(store: &mut Store, page: &mut Page, room: [f32; 4], id: &str, coming: f32) {
    let switches = store.switches();
    let Some(sandbox) = store.sandbox_of(id).cloned() else {
        page.ui().label(
            room,
            Text::Body,
            crate::i18n::text("permissions-after-install"),
            Role::TextSoft,
            Align::Left,
        );
        store.content_is(1, 1);
        return;
    };

    let height = page.metric(Metric::RowHeight) * 0.92;
    let heading = page.line(Text::Caption) * 1.5;
    let gap = page.metric(Metric::Gap);
    let rest = sandbox.also_asked();
    let asked_room = if rest.is_empty() {
        0.0
    } else {
        page.line(Text::Caption) * 2.4
    };
    let body = [
        room[0],
        room[1],
        room[2],
        (room[3] - asked_room).max(height),
    ];

    // Every switch is one step, and the row above them all puts everything
    // back. Headings are drawn between them and are not stepped on to.
    let deep = switches.len() + 1;
    let fits = (body[3] / height).floor().max(1.0) as usize;
    if !sandbox.touched() && store.content == 0 {
        // With no override to reset, the explanatory first row is not a
        // control. Put the light on the first switch instead of painting a
        // disabled sentence like the page's primary action.
        store.content = 1;
    }
    store.content_is(deep, fits.saturating_sub(1).max(1));

    let from = store.content_top.min(deep.saturating_sub(1));
    let band = store.band == Band::Content;
    let mut top = body[1];
    let mut group: Option<Group> = None;
    let bottom = body[1] + body[3];

    for step in from..deep {
        if step > 0 {
            let toggle = switches[step - 1];
            if group != Some(toggle.group) {
                if top + heading + height > bottom {
                    break;
                }
                let ui = page.ui();
                ui.label_tinted(
                    [body[0], top, body[2], heading],
                    Text::Caption,
                    toggle.group.title(),
                    ui.tinted(Role::TextSoft, coming * 0.7),
                    Align::Left,
                );
                top += heading;
                group = Some(toggle.group);
            }
        }
        if top + height > bottom {
            break;
        }
        let rect = [body[0], top, body[2], height];
        if band && step == store.content {
            page.glide(rect, coming);
        }
        if step == 0 {
            reset_row(store, page, rect, sandbox.touched(), coming);
        } else {
            switch_row(store, page, rect, switches[step - 1], &sandbox, coming);
        }
        // Until an override exists, the first row is an explanation rather
        // than a reset button. The controller can still read it, while a
        // pointer only gets a hand over controls that can act.
        if step > 0 || sandbox.touched() {
            page.ui().spot(CONTENT_SPOT + step as u32, rect);
        }
        top += height;
    }

    if !rest.is_empty() {
        let caption = page.line(Text::Caption);
        let said = rest.join(", ");
        let line = one_line(page, Text::Caption, &said, room[2]);
        let ui = page.ui();
        ui.label_tinted(
            [room[0], bottom + gap * 0.4, room[2], caption],
            Text::Caption,
            crate::i18n::text("permissions-also-asked-for"),
            ui.tinted(Role::TextSoft, coming * 0.7),
            Align::Left,
        );
        ui.label_tinted(
            [room[0], bottom + gap * 0.4 + caption, room[2], caption],
            Text::Caption,
            &line,
            ui.tinted(Role::TextSoft, coming * 0.55),
            Align::Left,
        );
    }
}

fn reset_row(store: &mut Store, page: &mut Page, rect: [f32; 4], touched: bool, coming: f32) {
    let pad = page.metric(Metric::RowPadding);
    let mark = page.metric(Metric::ItemIcon);
    let icons = page.icons();
    let lit = store.band == Band::Content && store.content == 0;
    let press = store.anim.press(lit);
    let rect = lxb_app::lxb_toolkit::motion::pressed(rect, press.through());
    let ui = page.ui();
    ui.icon_tinted(
        [rect[0], rect[1] + (rect[3] - mark) * 0.5, mark, mark],
        "refresh",
        icons,
        if touched {
            Role::Accent
        } else {
            Role::TextSoft
        },
        coming,
    );
    ui.label_tinted(
        [rect[0] + mark + pad, rect[1], rect[2] - mark - pad, rect[3]],
        Text::Body,
        if touched {
            crate::i18n::text("permissions-put-back")
        } else {
            crate::i18n::text("permissions-unchanged")
        },
        ui.tinted(if touched { Role::Text } else { Role::TextSoft }, coming),
        Align::Left,
    );
}

fn switch_row(
    store: &mut Store,
    page: &mut Page,
    rect: [f32; 4],
    toggle: &crate::sandbox::Toggle,
    sandbox: &crate::sandbox::Sandbox,
    coming: f32,
) {
    let standing = sandbox.standing(toggle);
    let pad = page.metric(Metric::RowPadding);
    let body_line = page.line(Text::Body);
    let caption = page.line(Text::Caption);
    let lit = store.band == Band::Content
        && store
            .switches()
            .get(store.content.saturating_sub(1))
            .is_some_and(|one| one.key == toggle.key);
    let press = store.anim.press(lit);
    let rect = lxb_app::lxb_toolkit::motion::pressed(rect, press.through());

    let switch_width = page.scaled(46.0);
    let switch_height = page.scaled(24.0);
    // The title and what it means on one line, because there are twenty of
    // these and a page that showed three of them at a time would be a page
    // nobody scrolls to the end of.
    let said = (rect[2] - switch_width - pad * 2.0).max(page.scaled(120.0));
    let titled = (said * 0.34).max(
        page.measure(Text::Body, crate::i18n::text(toggle.title))
            .min(said * 0.5),
    );
    let note = one_line(
        page,
        Text::Caption,
        crate::i18n::text(toggle.note),
        said - titled - pad,
    );

    let ui = page.ui();
    let _ = body_line;
    ui.label_tinted(
        [rect[0], rect[1], titled, rect[3]],
        Text::Body,
        crate::i18n::text(toggle.title),
        ui.tinted(Role::Text, coming),
        Align::Left,
    );
    ui.label_tinted(
        [
            rect[0] + titled + pad,
            rect[1],
            said - titled - pad,
            rect[3],
        ],
        Text::Caption,
        &note,
        ui.tinted(Role::TextSoft, coming * 0.85),
        Align::Left,
    );
    let _ = caption;

    // The switch: a track with a bead in it, at one end or the other. It is
    // the accent when it is on, and it carries a ring when this machine's
    // owner is the reason it stands where it does.
    let track = [
        rect[0] + rect[2] - switch_width,
        rect[1] + (rect[3] - switch_height) * 0.5,
        switch_width,
        switch_height,
    ];
    let on = standing.on();
    ui.chip(
        track,
        ui.tinted(
            if on { Role::Accent } else { Role::Glass },
            if on { 0.85 } else { 0.6 } * coming,
        ),
    );
    let bead = switch_height - ui.s(6.0);
    ui.chip(
        [
            if on {
                track[0] + track[2] - bead - ui.s(3.0)
            } else {
                track[0] + ui.s(3.0)
            },
            track[1] + ui.s(3.0),
            bead,
            bead,
        ],
        ui.tinted(Role::Text, coming),
    );
    if standing.changed() {
        let ring = ui.s(4.0);
        ui.chip(
            [
                track[0] - ring * 1.8,
                track[1] + (track[3] - ring) * 0.5,
                ring,
                ring,
            ],
            ui.tinted(Role::Accent, coming),
        );
    }
    let _ = standing;
    let _ = Standing::Asked;
}

/// Somewhere else to read about it.
fn links(store: &mut Store, page: &mut Page, room: [f32; 4], id: &str, coming: f32) {
    let links = store.links(id);
    let height = page.metric(Metric::RowHeight) * 1.1;
    let fits = (room[3] / height).floor().max(1.0) as usize;
    store.content_is(links.len().max(1), fits);

    let pad = page.metric(Metric::RowPadding);
    let mark = page.metric(Metric::ItemIcon);
    let band = store.band == Band::Content;
    let from = store.content_top.min(links.len().saturating_sub(1));

    for (offset, (kind, url)) in links.iter().enumerate().skip(from).take(fits) {
        let rect = [
            room[0],
            room[1] + height * (offset - from) as f32,
            room[2],
            height,
        ];
        let lit = band && offset == store.content;
        if lit {
            page.glide(rect, coming);
        }
        let press = store.anim.press(lit);
        let sunk = lxb_app::lxb_toolkit::motion::pressed(rect, press.through());
        let shown = one_line(page, Text::Caption, url, rect[2] - mark - pad * 2.0);
        let icons = page.icons();
        let ui = page.ui();
        ui.spot(CONTENT_SPOT + offset as u32, rect);
        ui.icon_tinted(
            [sunk[0], sunk[1] + (sunk[3] - mark) * 0.5, mark, mark],
            kind.glyph(),
            icons,
            Role::AccentSoft,
            coming,
        );
        let line = ui.line(Text::Body);
        let caption = ui.line(Text::Caption);
        let top = sunk[1] + (sunk[3] - line - caption) * 0.5;
        ui.label_tinted(
            [sunk[0] + mark + pad, top, sunk[2] - mark - pad, line],
            Text::Body,
            kind.title(),
            ui.tinted(Role::Text, coming),
            Align::Left,
        );
        ui.label_tinted(
            [
                sunk[0] + mark + pad,
                top + line,
                sunk[2] - mark - pad,
                caption,
            ],
            Text::Caption,
            &shown,
            ui.tinted(Role::TextSoft, coming * 0.8),
            Align::Left,
        );
    }
}

/// One repository.
pub fn repository(store: &mut Store, page: &mut Page, room: [f32; 4], name: &str, scope: Scope) {
    let coming = store.anim.arriving();
    let buttons = store.repository_buttons(name, scope);

    let Some(remote) = store.machine.remote(name, scope).cloned() else {
        page.ui().label(
            room,
            Text::Title,
            crate::i18n::text("repository-no-longer-configured"),
            Role::TextSoft,
            Align::Left,
        );
        return;
    };

    let offers = store.catalogue.count_from(&remote.name);
    let installed = store.machine.installed_from(&remote.name, remote.scope);

    let gap = page.metric(Metric::Gap);
    let mark = page.scaled(80.0);
    let title_line = page.line(Text::Display);
    let body_line = page.line(Text::Body);
    let caption = page.line(Text::Caption);
    let icons = page.icons();

    let mut said = vec![
        remote.scope.title().to_string(),
        crate::message!("remote-offers", "offers" => offers),
        crate::message!("remote-installed-from", "installed" => installed),
    ];
    if remote.priority != 1 {
        said.push(crate::message!("remote-priority", "priority" => remote.priority));
    }

    let text_left = room[0] + mark + gap * 1.4;
    let text_width = (room[2] - mark - gap * 1.4).max(page.scaled(80.0));

    // Cut before they are drawn. Both are somebody else's strings — a
    // repository added by hand carries whatever address was typed, and a
    // .flatpakrepo can name a homepage of any length — and a label clips
    // nothing.
    let shown_name = one_line(page, Text::Display, remote.shown(), text_width);
    let shown_url = one_line(page, Text::Body, &remote.url, text_width);
    let shown_homepage = one_line(page, Text::Caption, &remote.homepage, text_width);
    let shown_said = one_line(page, Text::Caption, &said.join("  ·  "), text_width);

    let ui = page.ui();
    ui.icon_tinted(
        [room[0], room[1], mark, mark],
        if remote.disabled {
            "do-not-disturb"
        } else {
            "file-drive"
        },
        icons,
        if remote.disabled {
            Role::TextSoft
        } else {
            Role::Accent
        },
        coming,
    );
    let mut top = room[1];
    ui.label_tinted(
        [text_left, top, text_width, title_line],
        Text::Display,
        &shown_name,
        ui.tinted(Role::Text, coming),
        Align::Left,
    );
    top += title_line;
    ui.label_tinted(
        [text_left, top, text_width, body_line],
        Text::Body,
        &shown_url,
        ui.tinted(Role::TextSoft, coming),
        Align::Left,
    );
    top += body_line;
    ui.label_tinted(
        [text_left, top, text_width, caption],
        Text::Caption,
        &shown_said,
        ui.tinted(Role::TextSoft, coming * 0.8),
        Align::Left,
    );
    top += caption;
    if !remote.homepage.is_empty() {
        ui.label_tinted(
            [text_left, top, text_width, caption],
            Text::Caption,
            &shown_homepage,
            ui.tinted(Role::TextSoft, coming * 0.8),
            Align::Left,
        );
        top += caption;
    }
    if !remote.gpg_verify {
        ui.label_tinted(
            [text_left, top, text_width, caption],
            Text::Caption,
            crate::i18n::text("repository-not-signature-checked"),
            ui.tinted(Role::Danger, coming),
            Align::Left,
        );
        top += caption;
    }
    if remote.disabled {
        ui.label_tinted(
            [text_left, top, text_width, caption],
            Text::Caption,
            crate::i18n::text("repository-switched-off-note"),
            ui.tinted(Role::TextSoft, coming),
            Align::Left,
        );
        top += caption;
    }

    let under = room[1] + mark.max(top - room[1]) + gap * 1.4;
    let places = button_places(page, room, under, &buttons);
    if let Some(rect) = places.get(store.button) {
        page.glide(*rect, coming);
    }
    row_of_buttons(store, page, &buttons, &places, true, coming);

    if !remote.description.is_empty() {
        let under = places_bottom(&places, under) + gap * 1.4;
        let body = [
            room[0],
            under,
            (room[2] * 0.7).max(page.scaled(200.0)),
            (room[1] + room[3] - under).max(0.0),
        ];
        page.ui()
            .paragraph(body, &remote.description, Role::TextSoft);
    }
}

/// A repository about to be added.
pub fn adding(store: &mut Store, page: &mut Page, room: [f32; 4]) {
    let coming = store.anim.arriving();

    let title_line = page.line(Text::Display);
    let caption = page.line(Text::Caption);
    let gap = page.metric(Metric::Gap);
    let height = page.metric(Metric::RowHeight) * 1.35;
    let mark = page.metric(Metric::ItemIcon);
    let pad = page.metric(Metric::RowPadding);

    let ui = page.ui();
    ui.label_tinted(
        [room[0], room[1], room[2], title_line],
        Text::Display,
        crate::i18n::text("add-a-repository"),
        ui.tinted(Role::Text, coming),
        Align::Left,
    );
    ui.label_tinted(
        [room[0], room[1] + title_line, room[2], caption],
        Text::Caption,
        crate::i18n::text("repository-added-for-this-user"),
        ui.tinted(Role::TextSoft, coming),
        Align::Left,
    );

    let top = room[1] + title_line + caption + gap;
    let count = flatpak::KNOWN.len() + 1;
    // Written down as it is drawn, so that a click landing on a row is
    // answered against the rows that are really there.
    store.content_is(count, count);
    let here: Vec<bool> = flatpak::KNOWN
        .iter()
        .map(|known| store.repository_is_here(known.name, Scope::User))
        .collect();

    let chosen = store.content.min(count - 1);
    if let Some(rect) = (chosen < count).then(|| {
        [
            room[0],
            top + height * chosen as f32,
            (room[2] * 0.8).max(page.scaled(240.0)),
            height,
        ]
    }) {
        page.glide(rect, coming);
    }

    for (index, known) in flatpak::KNOWN.iter().enumerate() {
        let rect = [
            room[0],
            top + height * index as f32,
            (room[2] * 0.8).max(page.scaled(240.0)),
            height,
        ];
        let lit = index == chosen;
        let press = store.anim.press(lit);
        let sunk = lxb_app::lxb_toolkit::motion::pressed(rect, press.through());
        let icons = page.icons();
        let ui = page.ui();
        if !here[index] {
            ui.spot(CONTENT_SPOT + index as u32, rect);
        }
        ui.icon_tinted(
            [sunk[0], sunk[1] + (sunk[3] - mark) * 0.5, mark, mark],
            if here[index] {
                "chosen"
            } else {
                "setting-connect"
            },
            icons,
            if here[index] {
                Role::TextSoft
            } else {
                Role::Accent
            },
            coming,
        );
        let line = ui.line(Text::Body);
        let block = line + caption;
        let at = sunk[1] + (sunk[3] - block) * 0.5;
        let width = sunk[2] - mark - pad;
        ui.label_tinted(
            [sunk[0] + mark + pad, at, width, line],
            Text::Body,
            known.title,
            ui.tinted(
                if here[index] {
                    Role::TextSoft
                } else {
                    Role::Text
                },
                coming,
            ),
            Align::Left,
        );
        ui.label_tinted(
            [sunk[0] + mark + pad, at + line, width, caption],
            Text::Caption,
            if here[index] {
                crate::i18n::text("already-on-this-machine")
            } else {
                crate::i18n::text(known.note)
            },
            ui.tinted(Role::TextSoft, coming * 0.85),
            Align::Left,
        );
    }

    // The address anybody can type, under the four nobody should have to.
    let index = flatpak::KNOWN.len();
    let rect = [
        room[0],
        top + height * index as f32,
        (room[2] * 0.8).max(page.scaled(240.0)),
        height,
    ];
    let lit = index == chosen;
    let typing = store.adding.typing;
    let typed = store.adding.url.clone();
    let named = store.adding.name.clone();
    let seconds = page.seconds();
    let radius = page.metric(Metric::CardRadius);
    let press = store.anim.press(lit);
    // Where the cursor is, for whatever is going to raise a keyboard over it.
    // The window says a field inside it has the cursor and where that field
    // is, and without the second half a keyboard has nothing to keep clear of.
    if typing {
        page.text_at(rect);
    }
    let icons = page.icons();
    let ui = page.ui();
    ui.spot(CONTENT_SPOT + index as u32, rect);
    ui.control(rect, radius, press, rect[2], coming);
    ui.icon_tinted(
        [rect[0] + pad, rect[1] + (rect[3] - mark) * 0.5, mark, mark],
        "setting-typed",
        icons,
        // Ink, not the accent: a mark in the accent on this page's own accent
        // ground is a mark that is there and cannot be read. What says the row
        // is being written into is how bright the mark is.
        Role::Text,
        coming * if typing { 1.0 } else { 0.62 },
    );
    let written = [
        rect[0] + pad * 2.0 + mark,
        rect[1],
        rect[2] - pad * 3.0 - mark,
        rect[3],
    ];
    let line = ui.line(Text::Body);
    let block = line + caption;
    let at = written[1] + (written[3] - block) * 0.5;
    if typed.is_empty() {
        ui.label_tinted(
            [written[0], at, written[2], line],
            Text::Body,
            if typing {
                crate::i18n::text("repository-address-note")
            } else {
                crate::i18n::text("type-an-address")
            },
            ui.tinted(Role::TextSoft, coming),
            Align::Left,
        );
    } else {
        ui.label_tinted(
            [written[0], at, written[2], line],
            Text::Body,
            &typed,
            ui.tinted(Role::Text, coming),
            Align::Left,
        );
    }
    ui.label_tinted(
        [written[0], at + line, written[2], caption],
        Text::Caption,
        &if !typing && typed.is_empty() {
            crate::i18n::text("repository-file-from-anywhere").to_string()
        } else if named.is_empty() {
            crate::i18n::text("repository-filed-under-the-file-name").to_string()
        } else {
            crate::message!("filed-under-name", "name" => (named).to_string())
        },
        ui.tinted(Role::TextSoft, coming * 0.8),
        Align::Left,
    );
    // The bar blinks only while something is really being written into, and
    // never on a control that is merely the one in hand.
    if typing && seconds.fract() < 0.55 {
        let width = ui.measure(Text::Body, &typed);
        let thick = ui.s(2.0).max(1.0);
        ui.rule(
            [
                written[0] + width + ui.s(3.0),
                at + line * 0.15,
                thick,
                line * 0.7,
            ],
            Role::Accent,
        );
    }
}

/// The handful of facts that decide whether somebody presses Install.
fn facts_of(
    store: &Store,
    listing: Option<&Listing>,
    installed: Option<&flatpak::Installed>,
) -> Vec<Fact> {
    let machine = &store.machine;
    let mut said = Vec::new();
    match installed {
        Some(one) => {
            if !one.version.is_empty() {
                said.push(Fact::plain(
                    crate::message!("version-number", "version" => one.version.to_string()),
                ));
            }
            said.push(Fact::accented(
                crate::message!("size-on-disk", "size" => flatpak::size(one.size)),
            ));
            said.push(Fact::plain(
                crate::message!("installed-for-scope", "scope" => one.scope.title().to_lowercase()),
            ));
            // The branch only where it is not the one everything is on, and
            // the origin always: an application installed from somewhere that
            // is not the remote offering it now is worth seeing.
            if !one.branch.is_empty() && one.branch != "stable" {
                said.push(Fact::plain(one.branch.clone()));
            }
            if !one.origin.is_empty() {
                said.push(Fact::plain(machine.remote_title(&one.origin)));
            }
            if !one.runtime.is_empty() {
                said.push(Fact::plain(
                    crate::message!("runs-on-runtime", "runtime" => short_runtime(&one.runtime)),
                ));
            }
            // An update needs no authorization under flatpak's own policy —
            // the commit is signed, and unattended updates would be impossible
            // otherwise — so on nearly every machine nothing is said here. On
            // one whose owner has locked updates down, it is.
            if one.scope.goes_through_the_helper() && flatpak::will_ask(flatpak::Act::Update) {
                said.push(Fact::warning(crate::i18n::text("authorization-required")));
            }
        }
        None => {
            if let Some(listing) = listing {
                if !listing.version.is_empty() {
                    said.push(Fact::plain(
                        crate::message!("version-number", "version" => listing.version.to_string()),
                    ));
                }
                // What it would really cost, once the worker has been out and
                // resolved it. That number counts every runtime and extension
                // this machine does not already have, which is the difference
                // between 828 kB and 759 MB.
                match store.weights.get(&listing.id) {
                    Some((download, installed)) => said.push(Fact::accented(crate::message!("download-and-on-disk", "download" => flatpak::size(*download), "size" => flatpak::size(*installed)))),
                    // Only while somebody is actually out working it out. A
                    // page that said so for ever would be a page waiting on
                    // nothing.
                    None if store.weighing(&listing.id) => {
                        said.push(Fact::plain(crate::i18n::text("calculating-download-size")))
                    }
                    None => {}
                }
                let scope = machine.install_scope(&listing.remote);
                said.push(Fact::plain(
                    crate::message!("installs-for-scope", "scope" => scope.title().to_lowercase()),
                ));
                // Only where this machine will really ask. See
                // `flatpak::will_ask`: "system" is not the answer, and saying
                // it was put this warning on every page of a machine whose
                // owner is never asked for anything.
                if scope.goes_through_the_helper() && flatpak::will_ask(flatpak::Act::Install) {
                    said.push(Fact::warning(crate::i18n::text("authorization-required")));
                }
                if !listing.runtime.is_empty() {
                    said.push(Fact::plain(crate::message!("runs-on-runtime", "runtime" => short_runtime(&listing.runtime))));
                }
            }
        }
    }
    // What people gave it, where anybody has. It is here because a store that
    // can put a shelf in order by a rating and never shows one is a store
    // hiding the number it sorted by — and because the plain average is the
    // one worth reading, while what the shelf is ordered by is that average
    // pulled towards the middle. See `ratings::Rating`.
    let id = listing
        .map(|one| one.id.as_str())
        .or(installed.map(|one| one.id.as_str()));
    if let Some(rated) = id.and_then(|id| store.ratings.of(id)) {
        let reviews = rated.reviews();
        said.push(Fact::plain(crate::message!("rating-reviews", "rating" => lxb_app::lxb_toolkit::i18n::decimal(format!("{:.1}", rated.mean())), "reviews" => reviews)));
    }
    if let Some(listing) = listing {
        if !listing.license.is_empty() {
            said.push(Fact::plain(license_title(&listing.license)));
        }
        if installed.is_none() {
            said.push(Fact::plain(machine.remote_title(&listing.remote)));
        }
    }
    said
}

/// AppStream allows custom license references to carry their explanatory URL
/// after an equals sign. That is useful source data but terrible shelf copy;
/// name the license class here and leave the project's URLs to the Links tab.
fn license_title(license: &str) -> String {
    let reference = license.split('=').next().unwrap_or(license).trim();
    match reference.to_ascii_lowercase().as_str() {
        "licenseref-proprietary" => crate::i18n::text("proprietary-license").into(),
        "licenseref-free" => crate::i18n::text("free-software-license").into(),
        one if one.starts_with("licenseref-") => crate::i18n::text("custom-license").into(),
        _ => license.to_string(),
    }
}

/// `org.gnome.Platform/x86_64/49` said the way somebody would say it.
///
/// The last part of a runtime's name is nearly always `Platform`, which says
/// nothing — what somebody wants to know is whose platform it is. So the part
/// before it is the one taken, and the three everything runs on are called
/// what their projects call themselves.
fn short_runtime(runtime: &str) -> String {
    let mut parts = runtime.split('/');
    let name = parts.next().unwrap_or(runtime);
    let branch = parts.nth(1).unwrap_or_default();

    let mut named: Vec<&str> = name.split('.').collect();
    if named.len() > 1 && matches!(named.last(), Some(&"Platform") | Some(&"BaseApp")) {
        named.pop();
    }
    let short = match named.last().copied().unwrap_or(name) {
        "gnome" => "GNOME",
        "kde" => "KDE",
        "freedesktop" => "freedesktop",
        other => other,
    };
    if branch.is_empty() {
        short.to_string()
    } else {
        format!("{short} {branch}")
    }
}

/// What the buttons do on a detail page, which depends on where the light is.
pub fn hints(store: &Store, id: &str) -> Vec<crate::legend::Hint> {
    use crate::legend::{hint, Button};
    let back = hint(crate::i18n::text("back"), Button::Back);
    match store.band {
        Band::Buttons => vec![hint(crate::i18n::text("press"), Button::Accept), back],
        // Nothing on the tabs is pressed: a tab is read by stepping down into
        // it, and a legend naming Accept here would name a button that does
        // nothing.
        Band::Tabs => vec![back],
        Band::Content => match store.tab(id) {
            Tab::About | Tab::Changes => vec![back],
            Tab::Permissions => vec![hint(crate::i18n::text("switch"), Button::Accept), back],
            Tab::Links => vec![hint(crate::i18n::text("open"), Button::Accept), back],
        },
    }
}

/// What a click landed on, on any of the pages that are about one thing.
pub fn pressed(store: &mut Store, page: &mut Page) {
    let screen = store.screen.clone();
    let buttons = match &screen {
        crate::store::Screen::Detail { id } => store.detail_buttons(id),
        crate::store::Screen::Repository { name, scope } => store.repository_buttons(name, *scope),
        _ => Vec::new(),
    };
    for (index, button) in buttons.iter().enumerate() {
        if page.pressed(BUTTON_SPOT + index as u32) {
            store.band = Band::Buttons;
            store.button = index;
            let button = *button;
            store.press(page, button);
            return;
        }
    }
    if let crate::store::Screen::Detail { id } = &screen {
        let id = id.clone();
        for index in 0..store.tabs(&id).len() {
            if page.pressed(TAB_SPOT + index as u32) {
                store.band = Band::Tabs;
                store.tab = index;
                store.content = 0;
                store.content_top = 0;
                store.anim.listed_anew();
                return;
            }
        }
        let shots = store
            .catalogue
            .get(&id)
            .map(|listing| listing.screenshots.len())
            .unwrap_or(0);
        for offset in 0..shots {
            if page.pressed(SHOT_SPOT + 1 + offset as u32) {
                store.shot = offset;
                store.anim.crossed();
                return;
            }
        }
    }
    // A repository's page is buttons and nothing else, so nothing there
    // answers on a content row.
    if matches!(screen, crate::store::Screen::Repository { .. }) {
        return;
    }
    for index in 0..store.content_depth() {
        if page.pressed(CONTENT_SPOT + index as u32) {
            store.band = Band::Content;
            store.content = index;

            // Links, permission switches and the known repository choices
            // behave like ordinary desktop controls: the click that points
            // at them also activates them. Reading rows and the custom URL
            // field only take focus.
            let activates = match &screen {
                crate::store::Screen::Detail { id } => {
                    matches!(store.tab(id), Tab::Permissions | Tab::Links)
                }
                crate::store::Screen::AddRepository => flatpak::KNOWN
                    .get(index)
                    .is_some_and(|known| !store.repository_is_here(known.name, Scope::User)),
                _ => false,
            };
            if activates {
                store.act(page, lxb_app::lxb_toolkit::input::Action::Accept);
            }
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{balanced_limit, license_title, rows_at};

    /// Seven facts that take two rows, the last of them short — the shape
    /// that put "Flathub" alone under a full line of chips.
    const SEVEN: [f32; 7] = [180.0, 150.0, 300.0, 190.0, 190.0, 170.0, 90.0];

    #[test]
    fn a_block_that_fits_on_one_row_is_left_on_one_row() {
        let (gap, room) = (8.0, 1600.0);
        assert_eq!(rows_at(&SEVEN, gap, room), 1);

        // The limit comes back as the row's own width rather than as the room
        // it was given, and that is the same picture: chips are laid from the
        // left, so a limit nothing reaches moves nothing.
        let limit = balanced_limit(&SEVEN, gap, room);
        assert_eq!(rows_at(&SEVEN, gap, limit), 1);
        let laid: f32 = SEVEN.iter().sum::<f32>() + gap * (SEVEN.len() - 1) as f32;
        assert!((limit - laid).abs() <= 1.0, "{limit} is not {laid}");
    }

    #[test]
    fn a_wrap_costs_no_extra_row_and_leaves_nothing_alone_on_the_last_one() {
        let (gap, room) = (8.0, 1180.0);
        let least = rows_at(&SEVEN, gap, room);
        assert_eq!(least, 2, "the case this was written for wraps once");

        let limit = balanced_limit(&SEVEN, gap, room);
        assert!(limit < room, "a full row and an orphan was left as it was");
        assert_eq!(
            rows_at(&SEVEN, gap, limit),
            least,
            "pulling the block in bought a row it did not have to"
        );

        // What each row really holds at that limit. The point of balancing is
        // that no row is left with one short chip under a full one.
        let mut rows = vec![0.0f32];
        for width in SEVEN {
            let used = rows.last_mut().expect("a row is always open");
            if *used > 0.0 && *used + gap + width > limit {
                rows.push(width);
            } else if *used > 0.0 {
                *used += gap + width;
            } else {
                *used = width;
            }
        }
        assert_eq!(rows.len(), least);
        let narrowest = rows.iter().copied().fold(f32::MAX, f32::min);
        assert!(
            narrowest > limit * 0.5,
            "a row was left less than half full: {rows:?} at {limit}"
        );
    }

    /// Two rows, the second shorter. Both have to end on the same edge.
    #[test]
    fn every_row_of_facts_ends_on_the_right_hand_edge() {
        use super::hang_right;
        let mut rects = [
            [0.0, 0.0, 100.0, 20.0],
            [110.0, 0.0, 80.0, 20.0],
            [200.0, 0.0, 60.0, 20.0],
            [0.0, 30.0, 90.0, 20.0],
            [100.0, 30.0, 50.0, 20.0],
        ];
        hang_right(&mut rects, 300.0);

        for last in [2, 4] {
            assert_eq!(
                rects[last][0] + rects[last][2],
                300.0,
                "row ending at {last}"
            );
        }
        // Shifted as a row: the gaps inside one are exactly what they were.
        assert_eq!(rects[1][0] - (rects[0][0] + rects[0][2]), 10.0);
        assert_eq!(rects[4][0] - (rects[3][0] + rects[3][2]), 10.0);
        // And the two rows moved by different amounts, which is the whole
        // reason for doing it a row at a time.
        assert_ne!(rects[0][0], rects[3][0]);
    }

    #[test]
    fn one_fact_too_wide_for_the_room_still_gets_a_row() {
        // A chip is capped at the room it has before it reaches any of this,
        // but nothing here may depend on that: a width past the limit has to
        // land somewhere rather than send the search looking for a row count
        // it can never reach.
        let widths = [400.0, 90.0];
        assert_eq!(rows_at(&widths, 8.0, 200.0), 2);
        assert!(balanced_limit(&widths, 8.0, 200.0).is_finite());
    }

    #[test]
    fn custom_license_urls_become_store_copy() {
        assert_eq!(
            license_title("LicenseRef-proprietary=https://example.invalid/notice"),
            crate::i18n::text("proprietary-license")
        );
        assert_eq!(
            license_title("LicenseRef-free"),
            crate::i18n::text("free-software-license")
        );
        assert_eq!(
            license_title("LicenseRef-project"),
            crate::i18n::text("custom-license")
        );
    }

    #[test]
    fn spdx_licenses_stay_exact() {
        assert_eq!(license_title("GPL-2.0+"), "GPL-2.0+");
    }
}
