//! What the store looks like.
//!
//! Two lists side by side: the shelves on the left, and what is on the chosen
//! shelf to the right of them. Everything is drawn in the toolkit's own
//! material — the light is one object crossing the page rather than a property
//! of a row, so it is laid down before the controls it is behind.
//!
//! Nothing here decides anything. It draws what `Store` says is true and
//! writes down where it put things, so that a pointer landing somewhere can be
//! told which row it landed on.
//!
//! One rule shapes the whole file. **Every quad is drawn before every text
//! run**, whatever order they were asked for in, so a panel drawn over a label
//! does not cover it — it cuts it in half. Nothing here draws a surface over
//! anything with words on it, and a row on its way in is faded rather than
//! masked.

use lxb_app::lxb_render::{Align, Fit, Spot, Written};
use lxb_app::lxb_toolkit::{
    material::{Overlay, Surface},
    metrics::Metric,
    palette::Role,
    typography::Text,
};
use lxb_app::Page;

use crate::flatpak;
use crate::motion::WORTH_DRAWING;
use crate::store::{Band, Column, Kind, Line, LineKind, Row, Screen, Shape, Shelf, Store};

/// Where a pointer can land. Kept well clear of the flow's own numbering,
/// which counts from zero in the order controls are drawn.
pub const SHELF_SPOT: u32 = 0x1000;
pub const ROW_SPOT: u32 = 0x2000;
pub const BUTTON_SPOT: u32 = 0x3000;
pub const TAB_SPOT: u32 = 0x4000;
pub const CONTENT_SPOT: u32 = 0x5000;
pub const SHOT_SPOT: u32 = 0x6000;
/// The empty space between rows is part of the scrollable pane too. These
/// background targets are laid down before the individual controls, so a card
/// or shelf still wins wherever one is actually under the pointer.
pub const SHELF_SCROLL_SPOT: u32 = 0x7000;
pub const LIST_SCROLL_SPOT: u32 = 0x7001;
/// The two bars a pointer can take hold of. See [`bar`].
pub const LIST_BAR_SPOT: u32 = 0x7002;

/// The search field. One of it, so it takes a number of its own rather than a
/// range: there is never more than one field on a page.
pub const FIELD_SPOT: u32 = 0x7004;

/// How bright the ink on a control that is not the one in hand is, which is
/// what the field's own mark has always used.
///
/// The wash under it is `detail::RESTING_WASH` — one number, because it is one
/// rule: a resting control steps back into the page so that the lit one reads
/// as the one in hand at a glance.
const RESTING_INK: f32 = 0.7;
pub const CONTENT_BAR_SPOT: u32 = 0x7003;

/// How long a picture takes to come up once it has arrived.
pub const PICTURE_FADE: f32 = 0.28;

pub fn draw(store: &mut Store, page: &mut Page) {
    let (pane, room) = window(page);

    if store.opening() {
        lay_the_window(page, pane);
        opening(store, page, room);
        return;
    }
    let nothing = store
        .machine
        .trouble
        .clone()
        .filter(|_| store.catalogue.listings.is_empty() && store.machine.installed.is_empty());
    if let Some(trouble) = nothing {
        lay_the_window(page, pane);
        nothing_here(page, room, &trouble);
        return;
    }

    match store.over() {
        None => {
            lay_the_window(page, pane);
            one_page(store, page, room, &Screen::Browse);
        }
        Some(over) if store.anim.filling_the_page() => {
            lay_the_window(page, pane);
            one_page(store, page, room, &over);
        }
        // The one page that lays the window down itself, and late. See
        // [`window`].
        Some(over) => crossing(store, page, pane, room, &over),
    }
}

/// The window's own pane, and the room inside it that a page is drawn in.
///
/// **This page lays its own window down** — `App::plain`, rather than letting
/// the toolkit put a pane under the page before the page function is even
/// called. A crossing steps the listing back with `Ui::recede` and fades it
/// with `Ui::recede_behind`, and **both act on everything drawn so far**: a
/// pane laid down before the page went with the page being left, so the whole
/// window dimmed and shrank as a page opened and popped back to full on the
/// one frame the crossing ended. That pop was the whole of the animation
/// anybody could see.
///
/// Drawn here it can be laid down *after* those two instead. It still ends up
/// underneath everything: a pane is on a lower layer than anything a page
/// draws, and the layers are composited in order whatever order they were
/// written in.
fn window(page: &mut Page) -> ([f32; 4], [f32; 4]) {
    let inset = page.metric(Metric::PanelInset);
    let pad = page.metric(Metric::PanelPadding);
    let pane = [
        inset,
        inset,
        (page.width() - 2.0 * inset).max(1.0),
        (page.height() - 2.0 * inset).max(1.0),
    ];
    let room = [
        pane[0] + pad,
        pane[1] + pad,
        (pane[2] - 2.0 * pad).max(1.0),
        (pane[3] - 2.0 * pad).max(1.0),
    ];
    // Said to the page, because everything that lays anything out asks it
    // where it is: `plain` leaves the whole window as the page's rectangle,
    // and the room inside the pane is what every other page here means by it.
    page.set_cursor(room);
    (pane, room)
}

fn lay_the_window(page: &mut Page, pane: [f32; 4]) {
    page.ui().pane(pane, Overlay::Dialog);
}

/// A page on its way out of the card that opened it, or back into it.
///
/// **One number, `Anim::grown`**, and everything here reads it: the rectangle
/// the page is drawn in, how far the listing behind it has stepped back and
/// faded, and how solid the page itself is. Back runs the same number the
/// other way, so the way out is the way in reversed rather than an animation
/// of its own — which is the whole of why there is one number and not two.
///
/// **The page is laid out at its full size and moved, never laid out small.**
/// Its corner is carried from the card's corner to the page's own, so what
/// shows through the card at the start is the top of the page — its mark and
/// the beginning of its name, which is what the card itself is showing. Laid
/// out into the growing rectangle instead, every line of it would rewrap on
/// every frame.
fn crossing(store: &mut Store, page: &mut Page, pane: [f32; 4], room: [f32; 4], over: &Screen) {
    let grown = store.anim.grown();
    let arriving = store.anim.arriving();

    one_page(store, page, room, &Screen::Browse);
    // Where the card is *now*, asked after the listing has been laid out and
    // not before: a window resized while a page was open would otherwise
    // shrink it back into a rectangle that has since moved.
    //
    // A page opened with the listing never drawn — a fixture, or a press on
    // the very first frame — has no card to grow out of at all, and the whole
    // page stands in for one. That leaves a plain crossing rather than
    // nothing at all.
    let card = store.opened_from().unwrap_or(room);
    let stage = crate::motion::between(card, room, grown);

    // **The listing clears exactly as fast as the page becomes solid**, which
    // is well before the page has finished growing. The two are one number
    // read both ways round: what is going hands over to what is coming, and
    // the crossing spends the rest of itself as one page growing rather than
    // as two pages at half strength each — which is two pages of type over
    // one another, and neither of them readable.
    //
    // There is nothing to hide the one behind the other with. A sheet of
    // glass over the listing refracts the whole page whatever its tint says,
    // because a glass quad's material is not scaled by its own alpha, and it
    // goes out like a light on the frame the crossing ends.
    let wall = 1.0 - arriving;

    // **The listing stays on the screen and steps back**, which is the
    // toolkit's own gesture for a page under a panel, at the size of a whole
    // page change. Both of these act on everything drawn so far, which is why
    // it happens between the two pages rather than inside either: draw the
    // listing, step it back, draw the page over it.
    //
    // Faded to nothing rather than to the menu's dim — what stands over this
    // page is the whole of another one. The cut is the second half of
    // `recede_behind` and the important one: without it a card's name is
    // drawn straight through the page growing over it, because every quad of
    // a layer goes down before any of that layer's words.
    let ui = page.ui();
    ui.recede(grown, card);
    ui.recede_behind(stage, wall, arriving);
    // The window belongs to neither page, so it is laid down after the two
    // calls that stepped the listing back rather than before them. See
    // [`window`].
    lay_the_window(page, pane);

    let from = page.ui().written();
    one_page(store, page, [stage[0], stage[1], room[2], room[3]], over);
    let to = page.ui().written();
    page.ui().cut_between(from, to, stage);
}

/// One whole page, from its head to the legend along its foot.
///
/// Told which screen to draw rather than reading it off the store, because
/// during a crossing there are two of them on the screen at once and only one
/// of them is the screen the presses go to.
fn one_page(store: &mut Store, page: &mut Page, room: [f32; 4], screen: &Screen) {
    // The browsing page is the one with a panel standing on the floor, so its
    // foot is only as wide as the listing beside it: the sidebar runs the
    // whole height of the window, and the legend is written to the right of
    // it. Everywhere else the foot has the width of the page.
    let split = matches!(screen, Screen::Browse).then(|| split_the_page(page, room));
    let foot = match &split {
        Some((_, listing)) => *listing,
        None => room,
    };
    // Where the foot of the page begins is worked out now and drawn at the
    // end. The two meet: a row leaving the bottom of the listing is drawn as
    // far as the legend, and the legend's own pictures of buttons are quads —
    // laid down first, they would be buried under a ghost card.
    let below = foot_top(store, page, foot);

    match screen {
        Screen::Browse => {
            let (panel, listing) = split.unwrap_or((room, room));
            let listing = [
                listing[0],
                listing[1],
                listing[2],
                (below - listing[1]).max(0.0),
            ];
            browse(store, page, panel, listing);
        }
        Screen::Detail { id } => {
            let room = [room[0], room[1], room[2], (below - room[1]).max(0.0)];
            crate::detail::application(store, page, room, id);
        }
        Screen::Repository { name, scope } => {
            let room = [room[0], room[1], room[2], (below - room[1]).max(0.0)];
            crate::detail::repository(store, page, room, name, *scope);
        }
        Screen::AddRepository => {
            let room = [room[0], room[1], room[2], (below - room[1]).max(0.0)];
            crate::detail::adding(store, page, room);
        }
    }
    footer(store, page, foot, screen);
}

/// The top of what runs along the bottom of the page, and so the bottom of
/// everything else on it. See [`footer`], which draws it.
fn foot_top(store: &Store, page: &mut Page, room: [f32; 4]) -> f32 {
    let bottom = room[1] + room[3];
    let caption = page.line(Text::Caption);
    let gap = page.metric(Metric::Gap);
    if store.running.is_some() {
        return bottom - (caption * 2.0 + gap) - gap;
    }
    let tall = crate::legend::height(page).max(caption);
    bottom - tall - gap
}

/// Where the panel of shelves stands, and what is left for the listing.
///
/// Worked out before anything is drawn, because the foot of the page needs it
/// too: what runs along the bottom belongs to the listing, not to the panel.
fn split_the_page(page: &mut Page, room: [f32; 4]) -> ([f32; 4], [f32; 4]) {
    // **Every margin on this page is the same margin.** The panel stands one
    // in from the left, the listing ends one in from the right, and this is
    // the space between the two. The toolkit's `ColumnSpacing` is the XMB's —
    // two hundred points, five times the page's own margin — and between a
    // panel and a grid it reads as a hole somebody forgot to fill rather than
    // as a margin.
    let gap = page.metric(Metric::PanelPadding);
    // Wide enough for the longest shelf name and its count side by side.
    // Narrower than that and Repositories is drawn as Repositori…, which reads
    // as a fault; wider and it is taking room from the grid for nothing.
    //
    // Measured rather than fixed, because "the longest shelf name" is a
    // different length in every language: 348 was the room English wants, and
    // Polish says Zainstalowane where English says Installed. The panel asks
    // the font how wide the names it is about to draw really are and takes
    // that much, inside the same bounds as before — so English is drawn
    // exactly as it was, and no language is drawn with its shelves cut.
    let names = crate::store::shelves()
        .into_iter()
        .map(|shelf| page.measure(Text::Body, shelf.short_title()))
        .fold(0.0f32, f32::max);
    // The same three pieces a shelf row is built from — see [`shelves`] — plus
    // the padding the panel keeps inside its own glass, which is what
    // [`shelf_room`] takes off before a row is laid out at all.
    let mark = page.metric(Metric::ItemIcon);
    let pad = page.metric(Metric::RowPadding) * 0.7;
    let tally = page.measure(Text::Caption, "888") + pad * 1.6;
    let wanted = names + mark + tally + pad * 2.6 + page.metric(Metric::PanelPadding);
    let width = page
        .scaled(348.0)
        .max(wanted)
        .min(room[2] * 0.32)
        .max(page.scaled(150.0));
    (
        [room[0], room[1], width, room[3]],
        [
            room[0] + width + gap,
            room[1],
            (room[2] - width - gap).max(page.scaled(120.0)),
            room[3],
        ],
    )
}

/// The one wait with nothing at all behind it.
///
/// A mark that breathes and a traveller crossing a track, because there is no
/// honest number to show: the catalogue is 47 MB of XML and how far through it
/// is says nothing about how long is left.
fn opening(store: &mut Store, page: &mut Page, room: [f32; 4]) {
    let mark = page.scaled(88.0);
    let breath = store.anim.breath();
    let sweep = store.anim.sweep();
    let line = page.line(Text::Title);
    let body = page.line(Text::Body);
    let track = page.scaled(240.0).min(room[2] * 0.4);
    let thick = page.scaled(4.0);
    let icons = page.icons();

    let top = room[1] + room[3] * 0.30;
    let at = [room[0] + (room[2] - mark) * 0.5, top, mark, mark];

    let ui = page.ui();
    ui.glow(
        lxb_app::lxb_toolkit::motion::scaled_about_centre(at, 2.4),
        Role::Accent,
        0.10 * breath,
    );
    ui.icon_tinted(
        lxb_app::lxb_toolkit::motion::scaled_about_centre(at, breath),
        "search",
        icons,
        Role::Text,
        1.0,
    );
    ui.label(
        [room[0], top + mark * 1.25, room[2], line],
        Text::Title,
        crate::i18n::text("reading-what-is-on-offer"),
        Role::Text,
        Align::Centre,
    );
    ui.label(
        [room[0], top + mark * 1.25 + line, room[2], body],
        Text::Body,
        crate::i18n::text("shelf-catalogue-note"),
        Role::TextSoft,
        Align::Centre,
    );

    // A traveller a third of the way across a track: it says that something is
    // happening, and it does not pretend to say how much is left.
    let rail = [
        room[0] + (room[2] - track) * 0.5,
        top + mark * 1.25 + line + body * 1.8,
        track,
        thick,
    ];
    ui.chip(rail, ui.tinted(Role::Glass, 0.5));
    let run = track * 0.34;
    ui.chip(
        [rail[0] + (track - run) * sweep, rail[1], run, thick],
        ui.role(Role::Accent),
    );
}

fn nothing_here(page: &mut Page, room: [f32; 4], trouble: &str) {
    let mark = page.scaled(96.0);
    let icons = page.icons();
    let ui = page.ui();
    ui.icon(
        [
            room[0] + (room[2] - mark) * 0.5,
            room[1] + room[3] * 0.32,
            mark,
            mark,
        ],
        "setting-info",
        icons,
    );
    let line = ui.line(Text::Title);
    ui.label(
        [
            room[0],
            room[1] + room[3] * 0.32 + mark * 1.4,
            room[2],
            line,
        ],
        Text::Title,
        trouble,
        Role::Text,
        Align::Centre,
    );
}

/// The two halves of the browsing page, and the one light that crosses both.
///
/// Ordered as carefully as it is laid out. **Every quad is drawn before every
/// text run**, so within the quads the order is what decides what is on top of
/// what: the sidebar's own panel and the cards go down first, the light goes
/// down over them, and only then do the marks and the words. A light drawn
/// before a card would be buried by it.
fn browse(store: &mut Store, page: &mut Page, panel: [f32; 4], listing_at: [f32; 4]) {
    // The sidebar is a surface of its own rather than a column of rows on the
    // page: it is a different kind of thing from what is beside it — a place
    // to stand rather than something on offer — and nothing else says so.
    page.ui().card(panel, Surface::Sidebar, Role::Glass, 0.40);
    let inside = shelf_room(page, panel);
    let shelves_shape = shelves_fit(store, page, inside);

    // The listing: everything below the heading and above the legend. It ends
    // where it ends, because everything drawn in it is cut to it — see
    // [`cut_to`]. It kept a whole empty row at each end before that, for a
    // half-shown row to be drawn full-height in, and those rows were content
    // the window had room for and was not showing.
    let head = head_height(page, store);
    let (body, track) = beside_a_bar(
        page,
        [
            listing_at[0],
            listing_at[1] + head,
            listing_at[2],
            (listing_at[3] - head).max(0.0),
        ],
    );

    // Wheel input uses the pointer target from the last complete frame. Mark
    // the whole two scrollable panes before their rows, so gaps and the soft
    // edge remain scrollable while a concrete row still takes precedence.
    page.ui().spot(SHELF_SCROLL_SPOT, shelves_shape.list);
    page.ui()
        .spot(LIST_SCROLL_SPOT, [body[0], body[1], listing_at[2], body[3]]);

    // The panel, from the mark before its first shelf to the mark after its
    // last word, is cut to the room inside it. What a page draws between two
    // marks is one thing, and this one is a list standing in a pane of glass:
    // nothing in it may cross the glass's own edge.
    let panel_from = page.ui().written();

    // The shelf that is open, marked before the light is laid down. When the
    // light has moved off into the listing this is the only thing on the whole
    // page that says which shelf that listing came from, and a name in a
    // heavier ink was not enough to say it.
    let on_this_shelf = lxb_app::lxb_toolkit::motion::pressed(
        shelf_rect(inside, store.shelf, shelves_shape),
        store.anim.press(store.column == Column::Shelves).through(),
    );
    if store.column == Column::Listing {
        let ui = page.ui();
        let tint = ui.tinted(Role::Accent, 0.20);
        ui.chip(on_this_shelf, tint);
    }
    if store.column == Column::Shelves {
        light(store, page, on_this_shelf, on_this_shelf[3] * 0.5);
    }
    shelves(store, page, inside, shelves_shape);
    let panel_to = page.ui().written();
    let shelf_list = shelves_shape.list;
    // **Cut to the panel itself, and not through `cut_to`.** That one grows a
    // list by a card's height either side, which is the room a card needs to
    // lean and swell as it arrives without being clipped for it. A panel is
    // the opposite case: it is a list standing in a pane of glass and nothing
    // in it may cross the glass's own edge, so a hundred points of air to the
    // right of it is a hundred points of listing that the panel is allowed to
    // draw on.
    page.ui().cut_between(
        panel_from,
        panel_to,
        [panel[0], shelf_list[1], panel[2], shelf_list[3]],
    );

    // The search field's own surface, before the light that stands in it. It
    // is drawn here rather than with its words for the same reason the cards
    // are: a light laid down first would be buried by it.
    let field = (store.shelf() == Shelf::Search).then(|| {
        let rect = search_field(page, listing_at);
        let radius = page.metric(Metric::CardRadius);
        // Drawn resting even while the light is in it. Told it was lit,
        // `Ui::control` lays down a light of its own — and this page carries
        // its own, one that is cut to the field's shape rather than to a
        // capsule. Two lights, one of them a frame behind the other on the
        // way in, is the rounded shadow that stuck out past the corner.
        let resting = store.column != Column::Field;
        let ui = page.ui();
        // Something for a pointer to land on. Without it the one control on
        // this shelf was the one control on the page that a click went
        // straight through — the field could only be reached by walking to it.
        ui.spot(FIELD_SPOT, rect);
        ui.control(rect, radius, store.anim.press(false), rect[2], 1.0);
        // **A control that is not the one in hand steps back into the page**,
        // which is the rule the detail page's own buttons keep and this field
        // did not. Left bright it read as lit whether or not it was, and the
        // words in it stood at 1.2 to 1 against their own ground — measured,
        // not guessed. Washed towards `Role::Glass`, and with the ink below,
        // it is 3.7 to 1.
        if resting {
            ui.chip(rect, ui.tinted(Role::Glass, crate::detail::RESTING_WASH));
        }
        // Where the field is, told to the compositor along with the fact that
        // it has the cursor, so the shell's keyboard has something to stand
        // clear of. See `Store::take_typing`.
        page.text_at(rect);
        rect
    });

    // A light is cut to the shape of whatever it is sitting on: the field is
    // `CardRadius` round, and the toolkit's own selection — a capsule whatever
    // it is over — left its corners sticking out behind it. The light on a
    // shelf was laid down with the panel, and the one on a card is laid down
    // with the cards; each belongs inside whatever is cut with it.
    let radius = page.metric(Metric::CardRadius);
    if store.column == Column::Field {
        if let Some(rect) = field {
            light(store, page, rect, radius);
        }
    }

    let plan = plan_the_listing(store, page, body);
    // Where every card really is, told to the store: a press has to know
    // which rectangle the page it opens grows out of, and only drawing knows.
    store.cards_are(plan.cells.iter().map(|cell| (cell.row, cell.at)).collect());

    // The cards, then the light over them, then — after the heading, which is
    // not part of the listing and must not be cut with it — their words. Every
    // quad is drawn before every word, so the cards and the words are two
    // ranges of the frame and not one, and each is cut to the listing on its
    // own.
    let cards_from = page.ui().written();
    for cell in &plan.cells {
        if cell.kind == LineKind::Heading || cell.fade <= WORTH_DRAWING {
            continue;
        }
        let pointer_blocked = store.rows.get(cell.row).is_some_and(|row| {
            store.running.is_some() && row.kind.is_head() && !store.head_is_running(&row.kind)
        });
        let ui = page.ui();
        // A store card carries more information than an ordinary list row, so
        // its surface needs enough body to remain legible over a bright or busy
        // wallpaper. The hero is stronger again: its screenshot and display
        // type should read as one deliberate destination, not as loose content.
        let strength = match cell.kind {
            LineKind::Hero => 0.55,
            LineKind::Cells => 0.43,
            LineKind::Wide => 0.38,
            LineKind::Heading => 0.0,
        };
        ui.card(cell.at, Surface::Control, Role::Glass, strength * cell.fade);
        // A row is answerable to the pointer over the part of it that is
        // really there, and a row barely in the list at all is not answerable:
        // a card standing behind the heading must never take a press meant for
        // the heading.
        if let Some(seen) = pressable(cell.rect, body) {
            if !pointer_blocked {
                ui.spot(ROW_SPOT + cell.row as u32, seen);
            }
        }
    }
    if store.column == Column::Listing {
        if let Some(cell) = plan.cells.iter().find(|cell| cell.row == store.row) {
            light(store, page, cell.at, radius);
        }
    }
    let cards_to = page.ui().written();
    cut_to(page, cards_from, cards_to, body);

    heading(store, page, listing_at, store.shelf());
    if store.rows.is_empty() {
        let line = page.line(Text::Body);
        let word = empty_words(store, store.shelf());
        page.ui().label(
            [body[0], body[1] + line, body[2], line],
            Text::Body,
            &word,
            Role::TextSoft,
            Align::Left,
        );
        return;
    }
    let words_from = page.ui().written();
    for cell in &plan.cells {
        draw_cell(store, page, cell);
    }
    let words_to = page.ui().written();
    cut_to(page, words_from, words_to, body);

    // The fixed heading, shelf panel and footer are outside `body`, so only
    // the moving application list dissolves. This happens after its words are
    // written: the renderer's SOFTEN layer samples cards, icons and text as
    // one complete picture rather than leaving sharp type over blurred glass.
    page.ui().soft_edges(
        body,
        plan.edge_band,
        plan.edge_strength[0],
        plan.edge_strength[1],
    );

    // Last, over everything the listing dissolved: a bar is beside the
    // reading rather than in it, and nothing about how far down a list is
    // may be softened away at the ends of that list.
    let (at, run) = store.listing_bar().unzip();
    let laid = bar(page, track, at.zip(run), LIST_BAR_SPOT);
    store.listing_bar_is(laid.0, laid.1);
}

/// Take the channel a bar stands in out of a band, and answer what is left of
/// the band and where the channel went.
///
/// **Kept whether or not a bar is drawn.** What a page reserves must not
/// depend on what the user last had in their hands, or a grid would relayout
/// the moment somebody put a pad down and reached for a mouse.
pub fn beside_a_bar(page: &mut Page, room: [f32; 4]) -> ([f32; 4], [f32; 4]) {
    let rail = page.ui().scroll_bar_width();
    let gap = page.metric(Metric::Gap) * 0.6;
    let width = (room[2] - rail - gap).max(page.scaled(80.0));
    (
        [room[0], room[1], width, room[3]],
        [room[0] + width + gap, room[1], rail, room[3]],
    )
}

/// Draw a bar down the edge of a list, and answer the track and thumb a drag
/// will be measured against — nothing at all where none was drawn.
///
/// **Nothing is drawn for a pad.** A controller is already saying where in the
/// list it is, with the light, and a bar beside that would be a control
/// nothing on the pad can reach. Nor where the whole list fits: a thumb as
/// long as its own track is a control that cannot be moved.
///
/// The spot is grown a little either way. Eight points is enough of a bar to
/// see and to catch under a finger, and not enough to hit with a mouse in one
/// go.
pub fn bar(
    page: &mut Page,
    track: [f32; 4],
    showing: Option<(f32, f32)>,
    id: u32,
) -> ([f32; 4], f32) {
    let Some((at, run)) = showing else {
        return ([0.0; 4], 0.0);
    };
    if page.pad_in_hand() {
        return ([0.0; 4], 0.0);
    }
    let held = page.dragging(id).is_some();
    let reach = page.metric(Metric::Gap) * 0.6;
    let ui = page.ui();
    let thumb = ui.scroll_bar(track, at, run, held);
    ui.spot(
        id,
        [track[0] - reach, track[1], track[2] + reach * 2.0, track[3]],
    );
    (track, thumb[3])
}

/// Cut everything drawn between two marks to the top and the bottom of a list.
///
/// **This is what lets a list end where it ends.** Nothing else on this page
/// can crop, so before this a row shown in part had to be a row drawn in full
/// somewhere — and the somewhere was the heading above the listing and the
/// legend below it. A whole empty row was kept at each end to hold the
/// overspill, which cost the window a row of what it had room to show.
///
/// Only the two ends are cut. A list is as wide as it is, but the light on a
/// card carries a glow past its own edge, and a glow squared off against thin
/// air is a worse fault than the one this is fixing.
fn cut_to(page: &mut Page, from: Written, to: Written, list: [f32; 4]) {
    let air = card_height(page);
    page.ui().cut_between(
        from,
        to,
        [list[0] - air, list[1], list[2] + air * 2.0, list[3]],
    );
}

/// How much of a row a pointer can land on, where that is enough of it to be
/// worth landing on at all.
fn pressable(rect: [f32; 4], list: [f32; 4]) -> Option<[f32; 4]> {
    let top = rect[1].max(list[1]);
    let bottom = (rect[1] + rect[3]).min(list[1] + list[3]);
    let height = bottom - top;
    (height > rect[3] * 0.6).then_some([rect[0], top, rect[2], height])
}

/// Carry the light to a rectangle and draw it there, in that rectangle's own
/// shape.
///
/// This store runs its own rather than calling `Page::glide`, which draws a
/// capsule whatever it is over. See `motion::Light`.
fn light(store: &mut Store, page: &mut Page, rect: [f32; 4], radius: f32) {
    let dt = store.anim.dt;
    let at = store.anim.light.glide(rect, dt);
    // Where the light really is, said to the toolkit: a menu grows out of it,
    // and `Page` only records this for the rows and buttons it draws itself.
    // Every control on these shelves is laid out here, so without this a Sort
    // menu grew out of the top-left corner of the window.
    page.light_at(at);
    page.ui().lit(
        at,
        radius,
        lxb_app::lxb_toolkit::control::LIT_ROLE,
        at[2],
        1.0,
    );
}

/// How faint a shelf has got by the time it is [`PEEK`] out of its panel.
///
/// The narrow shelf panel keeps the compact row fade that belongs inside its
/// own glass. The wider application viewport uses the renderer's true blurred
/// soft edge instead, after its cards and words have been composited together.
pub const GHOST: f32 = 0.32;

/// How much of the row beyond each end of a list is left showing, as a share
/// of a card.
///
/// Not quite half of one, which is what it takes to be read as a row rather
/// than as a smudge: at a third, the slice of a shelf left showing at the foot
/// of the panel was the top two points of its mark and none of its name. Less
/// than that and it looks like something clipped by accident; much more and it
/// is a row the window had room for and is refusing to show.
///
/// It is cheaper than it looks. A list is cut to a whole number of rows and
/// two peeks, so a larger peek usually eats the air that was left over rather
/// than a row: at 1600×900 the panel holds the same eleven shelves at a third
/// as it does at this.
pub const PEEK: f32 = 0.45;

/// [`PEEK`] in points.
fn peek_room(page: &Page) -> f32 {
    card_height(page) * PEEK
}

/// How far out of the panel a shelf has to be before it is as faint as it
/// gets, as a share of the peek showing at the end of it.
///
/// Half as far again, so a shelf at the edge is still dimming as the last of
/// it goes. One that reached its faintest while a slice of it was still
/// showing would sit there at one opacity and then be gone, which is the blink
/// all of this is here to be rid of.
///
/// The listing next door has no use for it. It does not dim at all: it
/// dissolves, over [`EDGE_BAND`], into whatever is behind it.
const SHELF_FADE: f32 = 1.6;

/// Whether any part of a row is inside its list at all.
///
/// What decides whether a row is drawn. It used to be that a row too faint to
/// see was the one not worth drawing; now a row keeps some ink until the last
/// of it is cut away, so it is the geometry that says when there is nothing
/// left to draw.
fn any_of_it_shows(rect: [f32; 4], list: [f32; 4]) -> bool {
    rect[1] + rect[3] > list[1] && rect[1] < list[1] + list[3]
}

/// How much of a shelf row is drawn, wherever it has got to.
///
/// Full ink while it is all inside the list, and down to [`GHOST`] as it
/// leaves. What is outside the list is cut away rather than drawn over
/// whatever the list stops short of, so this is only what softens the edge —
/// it is not what keeps a row off the heading.
fn showing_in(rect: [f32; 4], list: [f32; 4], ramp: f32) -> f32 {
    let out = (list[1] - rect[1]).max(0.0) + (rect[1] + rect[3] - (list[1] + list[3])).max(0.0);
    let smoothstep = lxb_app::lxb_toolkit::motion::smoothstep;
    1.0 - (1.0 - GHOST) * smoothstep((out / ramp.max(1.0)).clamp(0.0, 1.0))
}

/// Where the shelves go inside their panel: the panel's own padding, on all
/// four sides.
///
/// Nothing may cross a pane of glass's own edge — a shelf's mark hanging off
/// the bottom of the panel is not a cue, it is a fault — and nothing does,
/// because everything drawn in the panel is cut to this. It used to keep a
/// whole empty row inside the padding at each end for a half-shown shelf to be
/// drawn full-height in, and that row was two shelves the panel had room for
/// and would not show.
fn shelf_room(page: &Page, panel: [f32; 4]) -> [f32; 4] {
    let pad = page.metric(Metric::PanelPadding) * 0.5;
    [
        panel[0] + pad,
        panel[1] + pad,
        (panel[2] - pad * 2.0).max(0.0),
        (panel[3] - pad * 2.0).max(0.0),
    ]
}

/// How tall one shelf is, how many of them fit, and where the panel is
/// scrolled to.
///
/// Every shelf used to be on the screen at once: fifteen of them sharing out
/// whatever room there was, which on any ordinary window left each of them
/// half the height the design language asks for. They are drawn at their
/// proper size now, and the panel carries them — which is what the mark at its
/// foot is for.
#[derive(Debug, Clone, Copy)]
struct Shelves {
    height: f32,
    /// Where the first shelf begins, before the panel is scrolled.
    top: f32,
    scroll: f32,
    /// What the panel's list is cut to: a peek, a whole number of shelves,
    /// and a peek. See [`shelves_fit`].
    list: [f32; 4],
}

fn shelves_fit(store: &mut Store, page: &Page, at: [f32; 4]) -> Shelves {
    let height = page.metric(Metric::RowHeight).min(at[3].max(1.0));
    // **The list is cut to a peek, a whole number of shelves, and a peek** —
    // not to the room inside the panel. The two are rarely the same, and the
    // difference is what a panel with a shelf and a half to spare does with
    // it: it draws the spare shelf whole and the one after it as a
    // seven-point hairline, which is not a slice of anything, it is a fault.
    // Cut to what the shelves really come to, the last of them is always a
    // slice and always the same slice, and what is left over is air at the two
    // ends, where it reads as the panel's own padding.
    let peek = height * PEEK;
    let fits = (((at[3] - peek * 2.0) / height).floor() as usize).max(1);
    store.shelves_are(fits, height);
    let room = (peek * 2.0 + height * fits as f32).min(at[3]);
    let air = (at[3] - room) * 0.5;
    Shelves {
        height,
        top: at[1] + air + peek,
        scroll: store.anim.shelves.at(),
        list: [at[0], at[1] + air, at[2], room],
    }
}

fn shelf_rect(at: [f32; 4], index: usize, shape: Shelves) -> [f32; 4] {
    [
        at[0],
        shape.top + shape.height * index as f32 - shape.scroll,
        at[2],
        shape.height,
    ]
}

fn shelves(store: &mut Store, page: &mut Page, at: [f32; 4], shape: Shelves) {
    // How far out of the panel a shelf has to be to be as faint as it gets.
    let ramp = shape.height * PEEK * SHELF_FADE;
    let list = shape.list;
    let all = store.shelves();
    let chosen = store.shelf;
    let first = store.shelf_top;
    let column = store.column;
    let updates = store.machine.updatable().len();
    // Applications, which is what the shelf beside the number lists. The
    // runtimes under them are counted on the updates shelf, where they are
    // about to be fetched, and nowhere else.
    let installed = store.machine.apps().count();
    let repositories = store.machine.listed_remotes().len();

    for (index, shelf) in all.iter().enumerate() {
        let rect = shelf_rect(at, index, shape);
        // A shelf on its way over an edge of the panel is cut by it and fades
        // as it goes, rather than being taken away whole. See `showing_in`.
        if !any_of_it_shows(rect, list) {
            continue;
        }
        let showing = showing_in(rect, list, ramp);
        let lit = column == Column::Shelves && index == chosen;
        // The shelf being looked at stays marked once the light has moved to
        // the listing, or nothing on the screen says which shelf that is.
        let active = index == chosen;
        let press = store.anim.press(lit);
        let sunk = lxb_app::lxb_toolkit::motion::pressed(rect, press.through());
        let tally = match shelf {
            Shelf::Updates if updates > 0 => Some(updates.to_string()),
            Shelf::Installed if installed > 0 => Some(installed.to_string()),
            Shelf::Repositories if repositories > 0 => Some(repositories.to_string()),
            _ => None,
        };
        // A separator needs something on both sides of it. Scrolled to where
        // the shelves it parts are off the top of the panel, it is a rule
        // ruling nothing — drawn across the head of the list, where the mark
        // saying there is more above it also stands.
        let parted = shelf.parted_from_the_one_above() && index > first;
        let mark = page.metric(Metric::ItemIcon);
        let pad = page.metric(Metric::RowPadding) * 0.7;

        // Room kept for the tally before the name is measured against what is
        // left, and a clear gap between the two: the longest name here fills
        // the panel almost exactly, and a number set down on top of its last
        // letter reads as a fault rather than as a count.
        let tally_room = match &tally {
            Some(tally) => page.measure(Text::Caption, tally) + pad * 1.6,
            None => 0.0,
        };
        let left = pad * 1.6 + mark;
        let words = (sunk[2] - left - pad - tally_room).max(pad);
        // Clipped, because a label is drawn whatever width it was given: the
        // longest shelf here is Education & Science, and left to itself it
        // runs out past the panel it is standing in.
        let title = one_line(page, Text::Body, shelf.short_title(), words);

        let icons = page.icons();
        let ui = page.ui();
        ui.spot(SHELF_SPOT + index as u32, rect);

        if parted && showing > 0.999 {
            let thick = ui.s(1.0).max(1.0);
            ui.rule(
                [rect[0] + pad, rect[1], rect[2] - pad * 2.0, thick],
                Role::Rim,
            );
        }

        // The mark of the shelf being looked at is a little larger as well as
        // accented: at a glance it is the size that says which one it is.
        let grown = if active { 1.06 } else { 1.0 };
        let mark_at = lxb_app::lxb_toolkit::motion::scaled_about_centre(
            [
                sunk[0] + pad * 0.8,
                sunk[1] + (sunk[3] - mark) * 0.5,
                mark,
                mark,
            ],
            grown,
        );
        // **A mark is drawn in ink, never in the accent.** The shelf whose
        // listing is open has an accent chip laid down behind it, and an
        // accent mark on an accent chip is a mark nobody can see. What says
        // which shelf is open is the chip and the mark's size; what the mark
        // has to do is be legible.
        ui.icon_tinted(mark_at, shelf.glyph(), icons, Role::Text, showing);
        let ink = if lit || active {
            Role::Text
        } else {
            Role::TextSoft
        };
        let tint = ui.tinted(ink, showing);
        ui.label_tinted(
            [sunk[0] + left, sunk[1], words, sunk[3]],
            Text::Body,
            &title,
            tint,
            Align::Left,
        );
        if let Some(tally) = tally {
            // The same rule as the mark: on the chip it is ink, and off it the
            // accent is what makes a count read as a count rather than as part
            // of the name.
            let tint = ui.tinted(if active { Role::Text } else { Role::AccentSoft }, showing);
            ui.label_tinted(
                [sunk[0], sunk[1], sunk[2] - pad, sunk[3]],
                Text::Caption,
                &tally,
                tint,
                Align::Right,
            );
        }
    }
}

/// One card of the grid, worked out and ready to draw.
///
/// Everything in flight is settled here rather than where it is drawn, so that
/// a card's own surface, the light on it, and the words in it are all at one
/// place on the page. They were three sums in three functions, and a card
/// still leaning in from the right had its words leaning while its glass stood
/// still.
struct Cell {
    row: usize,
    kind: LineKind,
    /// Where it belongs when nothing is moving. What a pointer is told about.
    rect: [f32; 4],
    /// Where it really is this frame: leaned in if it is still arriving, sunk
    /// if it is being pressed.
    at: [f32; 4],
    /// How far its entrance has come. Leaving the viewport is handled after
    /// the complete row is composited, so its glass, icon and words soften as
    /// one object rather than fading at different stages.
    fade: f32,
    lit: bool,
}

struct Plan {
    cells: Vec<Cell>,
    edge_band: f32,
    edge_strength: [f32; 2],
}

/// How wide the smallest card is allowed to be.
///
/// Three across a window this size, two on a narrower one, one on a narrow
/// one. It is the card that has a size, not the grid: a store told to draw
/// three columns come what may would draw three unreadable slivers on a
/// half-width window.
///
/// Read off what a card really has to hold rather than guessed: an icon, the
/// padding round it, a name and the pill at the end of the name. At three
/// hundred, "Adventure Wrench" and "AI Generated Game" both came out as five
/// letters and an ellipsis, which is a card that has stopped saying what it is
/// about.
const NARROWEST_CARD: f32 = 380.0;

/// The most cards across, however wide the window is.
///
/// Three. Past that a card is too narrow to say what an application is, and a
/// window wide enough for a fourth is wide enough for three roomy ones.
const WIDEST_GRID: usize = 3;

/// Lay the listing out, and tell the store what shape it came out.
fn plan_the_listing(store: &mut Store, page: &mut Page, body: [f32; 4]) -> Plan {
    let gap = page.metric(Metric::Gap) * 0.7;
    let columns = (((body[2] + gap) / (page.scaled(NARROWEST_CARD) + gap)).floor() as usize)
        .clamp(1, WIDEST_GRID);
    let tall = line_heights(page, body[2]);

    // Written down before anything is measured against it, so that the lines
    // this lays out are the lines the store moves the light over.
    let mut tops = Vec::new();
    let mut at = 0.0;
    for line in &store.lines {
        tops.push(at);
        at += tall.of(line.kind)
            + if line.kind == LineKind::Heading {
                0.0
            } else {
                gap
            };
    }
    // A peek of a line is held back at the top of the listing, and the store
    // is told about it so that whatever it scrolls to leaves it there. What is
    // left at the foot after the whole lines is the slice of the next one, and
    // it is never two slices: `fitting_lines` stops at the last line that
    // fits, so the line after it cannot fit whole as well.
    let peek = peek_room(page);
    let room = fitting_lines(store, &tall, body[3] - peek);
    store.shape_is(Shape {
        columns,
        room,
        tops: tops.clone(),
        peek,
        viewport: body[3],
        deep: at,
    });

    let scroll = store.anim.scroll;
    let edge_band = edge_band(page, body[3]);
    let edge_strength = edge_strengths(at, scroll, body[3], edge_band);
    let scale = page.scaled(1.0);
    let mut cells = Vec::new();
    let mut depth = 0;
    for (index, line) in store.lines.iter().enumerate() {
        let top = tops.get(index).copied().unwrap_or(0.0) - scroll;
        let height = tall.of(line.kind);
        let rect = [body[0], body[1] + top, body[2], height];
        if rect[1] > body[1] + body[3] {
            break;
        }
        // A line on its way over either edge is still cut exactly to the
        // listing. The complete clipped result is softened later; doing that
        // here would fade each quad and text run separately.
        if !any_of_it_shows(rect, body) {
            continue;
        }
        // A heading with nothing under it is the name of a section whose first
        // card is off the bottom of the page: a line of type left hanging.
        if line.kind == LineKind::Heading
            && !tall.fits_under(&store.lines, &tops, index, scroll, body)
        {
            break;
        }
        let across = line.rows.len().max(1);
        for (column, row) in line.rows.iter().enumerate() {
            let rect = match line.kind {
                LineKind::Cells => {
                    let width = (body[2] - gap * (columns - 1) as f32) / columns as f32;
                    [
                        body[0] + (width + gap) * column as f32,
                        body[1] + top,
                        width,
                        height,
                    ]
                }
                _ => [body[0], body[1] + top, body[2], height],
            };
            let coming = store.anim.row_in(depth, scale);
            let lit = store.column == Column::Listing && *row == store.row;
            let press = store.anim.press(lit);
            let at = lxb_app::lxb_toolkit::motion::pressed(coming.moved(rect), press.through());
            cells.push(Cell {
                row: *row,
                kind: line.kind,
                rect,
                at,
                // The post-composite edge treats the card, icon and words as
                // one picture. Uniformly fading the row here as well makes
                // text disappear before it has had a chance to soften.
                fade: coming.fade,
                lit,
            });
            let _ = across;
        }
        depth += 1;
    }
    Plan {
        cells,
        edge_band,
        edge_strength,
    }
}

/// How deep the dissolve at each end of the listing goes, as a share of a
/// card.
///
/// A card and a sixth, so the row at the edge is well into it before the row
/// above has begun. What the eye reads then is a list going out of focus over
/// a distance rather than one row that has been faded — which is the whole
/// difference between a soft edge and a cut one.
const EDGE_BAND: f32 = 1.15;

/// That in points, kept to a share of the window so a short one still has a
/// whole crisp card between its two ends.
fn edge_band(page: &Page, viewport_height: f32) -> f32 {
    (card_height(page) * EDGE_BAND)
        .min(viewport_height * 0.22)
        .min(((viewport_height - card_height(page)) * 0.5).max(0.0))
}

/// How much content really continues beyond each viewport edge.
///
/// Geometry rather than the selected line drives this, so the blur grows and
/// clears with the same spring as the cards instead of popping when `top`
/// changes. The first edge is top and the second is bottom.
fn edge_strengths(content: f32, scroll: f32, viewport: f32, band: f32) -> [f32; 2] {
    if band <= 0.0 {
        return [0.0; 2];
    }
    let smoothstep = lxb_app::lxb_toolkit::motion::smoothstep;
    let strength = |hidden: f32| smoothstep((hidden / band).clamp(0.0, 1.0));
    [
        strength(scroll.max(0.0)),
        strength((content - scroll - viewport).max(0.0)),
    ]
}

fn card_height(page: &Page) -> f32 {
    page.metric(Metric::RowHeight) * 1.5
}

/// The hero grows with the room it is given, then stops before it pushes every
/// other destination off an ordinary page. Below the split layout's threshold
/// it becomes a taller stacked card, leaving the screenshot and the words room
/// to keep their own proportions instead of squeezing beside each other.
fn hero_height(page: &Page, width: f32) -> f32 {
    let row = page.metric(Metric::RowHeight);
    if width >= page.scaled(HERO_SPLIT_AT) {
        (width * 0.27).clamp(row * 3.1, row * 4.2)
    } else {
        (width * 0.70).clamp(row * 4.2, row * 5.8)
    }
}

/// At this width the hero changes from a vertical story to screenshot beside
/// copy. Kept in points so it follows the rest of the toolkit under scaling.
const HERO_SPLIT_AT: f32 = 720.0;

/// How tall each kind of line is, measured once.
///
/// One place, because three of them ask — laying the tops out, drawing, and
/// working out how many fit — and three copies of the same sum are three
/// chances for the light to land where nothing was drawn.
#[derive(Debug, Clone, Copy)]
struct Heights {
    hero: f32,
    heading: f32,
    wide: f32,
    card: f32,
    gap: f32,
}

fn line_heights(page: &Page, width: f32) -> Heights {
    let gap = page.metric(Metric::Gap) * 0.7;
    Heights {
        hero: hero_height(page, width),
        heading: page.line(Text::Title) + page.line(Text::Caption) + gap,
        wide: page.metric(Metric::RowHeight) * 1.25,
        card: card_height(page),
        gap,
    }
}

impl Heights {
    fn of(&self, kind: LineKind) -> f32 {
        match kind {
            LineKind::Hero => self.hero,
            LineKind::Heading => self.heading,
            LineKind::Wide => self.wide,
            LineKind::Cells => self.card,
        }
    }

    /// Whether the line under this one is drawn at all.
    fn fits_under(
        &self,
        lines: &[Line],
        tops: &[f32],
        index: usize,
        scroll: f32,
        body: [f32; 4],
    ) -> bool {
        let Some(next) = lines.get(index + 1) else {
            return false;
        };
        let top = tops.get(index + 1).copied().unwrap_or(f32::MAX) - scroll;
        let rect = [body[0], body[1] + top, body[2], self.of(next.kind)];
        any_of_it_shows(rect, body)
    }
}

/// How many lines fit below the one at the top, which is what a page of Next
/// and Previous is and what keeps the chosen card on the screen.
fn fitting_lines(store: &Store, tall: &Heights, room: f32) -> usize {
    let mut used = 0.0;
    let mut count: usize = 0;
    for line in store.lines.iter().skip(store.top) {
        let height = tall.of(line.kind)
            + if line.kind == LineKind::Heading {
                0.0
            } else {
                tall.gap
            };
        if used + height > room {
            break;
        }
        used += height;
        count += 1;
    }
    // A heading is only worth a line of the page if what it names fits under
    // it. Counted here as well as drawn, so that Next and Previous step by
    // what is really on the screen.
    if store
        .lines
        .get(store.top + count.saturating_sub(1))
        .is_some_and(|line| line.kind == LineKind::Heading)
    {
        count -= 1;
    }
    count.max(1)
}

/// Where the search field is, on the one shelf that has one.
///
/// Worked out here rather than where it is drawn, because three things need
/// it: the surface it is drawn on, the light that stands in it, and the
/// compositor, which is told where the field is so an on-screen keyboard can
/// stand clear of it.
fn search_field(page: &Page, at: [f32; 4]) -> [f32; 4] {
    let line = page.line(Text::Title);
    let gap = page.metric(Metric::Gap);
    [
        at[0],
        at[1] + line + gap,
        at[2],
        page.metric(Metric::RowHeight),
    ]
}

/// From the top of the right-hand pane to where the listing's ghost stands.
fn head_height(page: &Page, store: &Store) -> f32 {
    let line = page.line(Text::Title);
    let gap = page.metric(Metric::Gap);
    match store.shelf() {
        Shelf::Search => line + gap * 1.5 + page.metric(Metric::RowHeight),
        _ => line + gap * 1.5,
    }
}

fn empty_words(store: &Store, shelf: Shelf) -> String {
    match shelf {
        Shelf::Home if store.flathub.waiting() => crate::i18n::text("flathub-asking").into(),
        // Two different empty pages, because they want two different answers.
        Shelf::Home if store.flathub.any() => {
            crate::i18n::text("flathub-nothing-offered-here").into()
        }
        Shelf::Home => crate::i18n::text("flathub-unreachable").into(),
        Shelf::Search if store.query.trim().is_empty() => crate::i18n::text("type-to-look").into(),
        Shelf::Search => crate::i18n::text("nothing-answers-to-that").into(),
        Shelf::Updates => crate::i18n::text("everything-up-to-date").into(),
        Shelf::Installed => crate::i18n::text("nothing-installed-yet").into(),
        Shelf::Repositories => crate::i18n::text("no-repository-configured").into(),
        Shelf::Section(_) => crate::i18n::text("remote-offers-nothing-here").into(),
    }
}

/// How many, and of what.
///
/// One noun per shelf, and the singular where there is one of it: "1
/// application" reads as a sentence where "1 applications" reads as a bug.
fn counted(shelf: Shelf, count: usize) -> String {
    match shelf {
        Shelf::Repositories => crate::message!("count-repositories", "count" => count),
        Shelf::Updates => crate::message!("count-updates-waiting", "count" => count),
        Shelf::Installed => crate::message!("count-apps-installed", "count" => count),
        Shelf::Search => crate::message!("count-apps-found", "count" => count),
        _ => crate::message!("count-apps", "count" => count),
    }
}

fn heading(store: &mut Store, page: &mut Page, at: [f32; 4], shelf: Shelf) {
    let line = page.line(Text::Title);
    let caption = page.line(Text::Caption);

    // How much of a long list is on the screen, said beside the heading rather
    // than under the last card: a card on its way in passes through the bottom
    // of the listing, and nothing there can be drawn over.
    // Everything on the shelf that is a *thing*, which on the updates shelf
    // means the runtimes under Application support as well as the
    // applications over them: eleven are going to be fetched, and a corner
    // that said seven was counting the half of the page it could see.
    let count = store
        .rows
        .iter()
        .filter(|row| {
            row.kind.is_app() || matches!(row.kind, Kind::Repo { .. } | Kind::Support { .. })
        })
        .count();
    // A total is worth saying about a shelf that is a list of things. Home is
    // four short lists of somebody else's choosing, and how many that comes to
    // is not a fact about anything.
    //
    // **And it says what it is counting.** A bare "432" beside a category is a
    // number with no noun: it could as easily be a size, a version or a
    // position in a list, and somebody who has to work out which is somebody
    // the page failed. What the noun is depends on the shelf, because these
    // are not all the same kind of four hundred.
    let mut shown = if count > 0 && shelf != Shelf::Home {
        counted(shelf, count)
    } else {
        String::new()
    };
    // What is worth saying about this shelf beyond how long it is. An
    // application nobody is going to fix again is the one thing an Installed
    // shelf has to say without being asked.
    let ending = store.machine.ending().len();
    if shelf == Shelf::Installed && ending > 0 {
        shown = crate::message!("shown-and-ending", "shown" => shown, "ending" => ending);
    }
    // And which way round they are, last because it is the one part that is
    // about the list rather than about what is on it. Said on the page rather
    // than only inside the menu: an order somebody has to press a button to
    // learn is an order they will assume is the only one there is.
    if store.can_be_ordered() && !shown.is_empty() {
        shown = format!("{shown}  ·  {}", store.showing_order().shown(shelf));
    }

    // The corner is hung off the right-hand margin and the name is hung off
    // the left, and they are drawn into the same rectangle: what stops one
    // running through the other is cutting this to whatever the name leaves.
    // Nothing is clipped by its box in this renderer, so a line too long for
    // its room is a line drawn straight over its neighbour.
    if !shown.is_empty() {
        let taken = page.measure(Text::Title, shelf.title()) + page.metric(Metric::Gap) * 2.0;
        shown = one_line(page, Text::Caption, &shown, (at[2] - taken).max(0.0));
    }

    let ui = page.ui();
    ui.label(
        [at[0], at[1], at[2], line],
        Text::Title,
        shelf.title(),
        Role::Text,
        Align::Left,
    );
    if !shown.is_empty() {
        ui.label(
            [at[0], at[1] + (line - caption) * 0.6, at[2], caption],
            Text::Caption,
            &shown,
            Role::TextSoft,
            Align::Right,
        );
    }

    if shelf != Shelf::Search {
        return;
    }

    let field = search_field(page, at);
    let pad = page.metric(Metric::RowPadding);
    let mark = page.metric(Metric::ItemIcon);
    let icons = page.icons();
    let typing = store.query.clone();
    let seconds = page.seconds();
    let standing_in = store.column == Column::Field;

    // The surface under the field was laid down with the cards; see `browse`.
    // Only what is written in it is drawn here.
    let ui = page.ui();
    ui.icon_tinted(
        [
            field[0] + pad,
            field[1] + (field[3] - mark) * 0.5,
            mark,
            mark,
        ],
        "search",
        icons,
        Role::Text,
        if standing_in { 1.0 } else { RESTING_INK },
    );
    let written = [
        field[0] + pad * 2.0 + mark,
        field[1],
        field[2] - pad * 3.0 - mark,
        field[3],
    ];
    if typing.is_empty() {
        // **Ink, quieted, rather than a quieter ink.** `Role::TextSoft` is cut
        // to read on the page's own ground and this is a control standing on
        // it: the same words came out at 1.2 to 1 here against 3.4 to 1 an inch
        // below, which is a placeholder nobody can see; it is 3.7 to 1 now.
        // The strength is the magnifier's beside it, because it is saying the
        // same thing — this field, and nothing typed into it yet.
        ui.label_tinted(
            written,
            Text::Body,
            crate::i18n::text("look-for-something"),
            ui.tinted(Role::Text, RESTING_INK),
            Align::Left,
        );
    } else {
        ui.label(written, Text::Body, &typing, Role::Text, Align::Left);
    }
    // A bar that blinks, so a field with nothing in it still says a key would
    // go here — and only while the light is standing in it, because a bar
    // blinking in a field nothing would reach is a lie. Its own second, not
    // the wallpaper's.
    if standing_in && seconds.fract() < 0.55 {
        let width = ui.measure(Text::Body, &typing);
        let thick = ui.s(2.0).max(1.0);
        let tall = ui.line(Text::Body) * 0.72;
        ui.rule(
            [
                written[0] + width + ui.s(3.0),
                written[1] + (written[3] - tall) * 0.5,
                thick,
                tall,
            ],
            Role::Accent,
        );
    }
}

/// One card, one head row, or one line of type.
///
/// Nothing here works out where anything is. The plan settled that, so that a
/// card's glass, the light on it and its own words cannot disagree.
fn draw_cell(store: &mut Store, page: &mut Page, cell: &Cell) {
    let Some(row) = store.rows.get(cell.row).cloned() else {
        return;
    };
    if cell.fade <= WORTH_DRAWING {
        return;
    }
    let rect = cell.at;
    let fade = cell.fade;

    if cell.kind == LineKind::Heading {
        let title = page.line(Text::Title);
        let caption = page.line(Text::Caption);
        let ui = page.ui();
        ui.label_tinted(
            [rect[0], rect[1], rect[2], title],
            Text::Title,
            &row.name,
            ui.tinted(Role::Text, fade),
            Align::Left,
        );
        if !row.summary.is_empty() {
            ui.label_tinted(
                [rect[0], rect[1] + title, rect[2], caption],
                Text::Caption,
                &row.summary,
                ui.tinted(Role::TextSoft, fade * 0.8),
                Align::Left,
            );
        }
        return;
    }

    if cell.kind == LineKind::Hero {
        draw_hero(store, page, &row, rect, fade);
        return;
    }

    // One padding, used on all four sides and between the mark and the words,
    // so a card has the same air round everything in it.
    let pad = page.metric(Metric::RowPadding) * 0.8;
    let radius = page.metric(Metric::CardRadius);
    let mark = (rect[3] - pad * 2.0).min(page.scaled(64.0));
    let icons = page.icons();

    // A head row that started what is running is the row that stops it.
    let running = store.head_is_running(&row.kind);
    let blocked = store.running.is_some() && row.kind.is_head() && !running;
    let name = if running {
        crate::i18n::text("stop").to_string()
    } else {
        row.name.clone()
    };
    let summary = if running {
        crate::i18n::text("stop-keeps-what-was-fetched").to_string()
    } else if blocked {
        crate::i18n::text("available-when-the-work-finishes").to_string()
    } else {
        row.summary.clone()
    };
    let glyph = if running { "do-not-disturb" } else { row.glyph };

    let name_line = page.line(Text::Body);
    let summary_line = page.line(Text::Caption);
    let text_left = rect[0] + pad * 2.0 + mark;
    let text_width = (rect[0] + rect[2] - pad - text_left).max(page.scaled(40.0));

    // Developer identity earns a line on an application card. When it is
    // present the summary gives one of its two lines back, keeping every card
    // in a row aligned without making the grid taller just for some publishers.
    let developer = row
        .kind
        .is_app()
        .then(|| row.developer.trim())
        .filter(|said| !said.is_empty())
        .map(|said| crate::message!("by-publisher", "publisher" => (said).to_string()));
    // Two lines of summary on a card and one across a head row, because a head
    // row is as wide as the page and a card is a third of it.
    let most = if cell.kind == LineKind::Wide || developer.is_some() {
        1
    } else {
        2
    };

    let stamp = stamp_of(&row);
    // Room for the pill, and a clear gap between it and the name it stands
    // beside. Fitted to its word and then set down against the name, a pill
    // reads as the last syllable of the name rather than as a mark on the row.
    let stamp_width = match &stamp {
        Some((word, _)) => page.measure(Text::Caption, word) + pad * 1.5,
        None => 0.0,
    };
    let stamp_gap = if stamp.is_some() { pad * 1.4 } else { 0.0 };
    let named = one_line(
        page,
        Text::Body,
        &name,
        (text_width - stamp_width - stamp_gap).max(page.scaled(30.0)),
    );

    let wrapped = page.ui().wrap(
        Text::Caption,
        &summary.replace(['\n', '\r'], " "),
        text_width,
    );
    let cut = wrapped.len() > most;
    let mut said: Vec<String> = wrapped.into_iter().take(most).collect();
    // A summary that ran on says so. Cut off mid-word with nothing after it,
    // it reads as a summary somebody forgot to finish.
    if cut && !said.is_empty() {
        let last = said.len() - 1;
        said[last] = shortened(page, Text::Caption, &said[last], text_width);
    }

    // An icon this remote never wrote to the disk, fetched the way a
    // screenshot is. Asked for here rather than when the listing was built, so
    // that a shelf of three thousand asks for the dozen that are on screen.
    let fetched = row
        .icon
        .is_none()
        .then(|| row.icon_url.clone())
        .flatten()
        .and_then(|url| {
            let at = store.art.fade(&url, PICTURE_FADE);
            store.art.picture(&url).map(|path| (path, at))
        });

    let ui = page.ui();
    let icon_at = [rect[0] + pad, rect[1] + (rect[3] - mark) * 0.5, mark, mark];
    let drawn = match (&row.icon, &fetched) {
        (Some(path), _) => ui.picture(icon_at, radius, path, Fit::Contain, fade),
        (None, Some((path, arriving))) => {
            ui.picture(icon_at, radius, path, Fit::Contain, fade * arriving)
        }
        (None, None) => false,
    };
    if !drawn {
        // Ink, not accent. A card is a pane of purple glass and the accent is
        // a purple; the two came out a few points apart in every channel but
        // blue, which is a mark that is there and cannot be read. A head row
        // wears its mark a little more strongly, because on a head row the
        // mark is half of what says what the row does.
        let strength = if row.kind.is_head() { 0.92 } else { 0.78 };
        ui.icon_tinted(icon_at, glyph, icons, Role::Text, strength * fade);
    }

    // **The block is as tall as a card of this kind can be, whether this one
    // fills it or not.** Measured to what this card happens to say, a name
    // beside a one-line summary sits half a line lower than the name next to
    // it — and so does the pill at the end of it, which is what made a row of
    // pills look scattered down the page.
    let block = name_line + summary_line * (most + usize::from(developer.is_some())) as f32;
    let top = rect[1] + (rect[3] - block) * 0.5;
    ui.label_tinted(
        [text_left, top, text_width, name_line],
        Text::Body,
        &named,
        // A product name is primary information even before the light reaches
        // it. Keeping it in full ink is the second half of the stronger card
        // surface above.
        ui.tinted(Role::Text, fade * if cell.lit { 1.0 } else { 0.94 }),
        Align::Left,
    );
    let mut line_at = top + name_line;
    if let Some(developer) = developer {
        let developer = one_line(page, Text::Caption, &developer, text_width);
        let ui = page.ui();
        ui.label_tinted(
            [text_left, line_at, text_width, summary_line],
            Text::Caption,
            &developer,
            ui.tinted(Role::Text, fade * 0.74),
            Align::Left,
        );
        line_at += summary_line;
    }
    let ui = page.ui();
    for (offset, line) in said.iter().enumerate() {
        ui.label_tinted(
            [
                text_left,
                line_at + summary_line * offset as f32,
                text_width,
                summary_line,
            ],
            Text::Caption,
            line,
            ui.tinted(Role::TextSoft, fade * 0.96),
            Align::Left,
        );
    }

    if let Some((word, role)) = stamp {
        let tall = summary_line * 1.5;
        badge(
            ui,
            [
                rect[0] + rect[2] - stamp_width - pad,
                top + (name_line - tall) * 0.5,
                stamp_width,
                tall,
            ],
            &word,
            role,
            fade,
        );
    }
}

/// Home's promoted application: one destination across the listing, with the
/// catalogue's own screenshot when it has one and the application's icon when
/// it does not. Wide pages put image and copy beside each other; narrow pages
/// stack them, so neither is reduced to a sliver.
fn draw_hero(store: &mut Store, page: &mut Page, row: &Row, rect: [f32; 4], fade: f32) {
    let pad = page.metric(Metric::RowPadding);
    let gap = page.metric(Metric::Gap) * 0.65;
    let radius = page.metric(Metric::CardRadius);
    let split = rect[2] >= page.scaled(HERO_SPLIT_AT);

    let (words, visual) = if split {
        let picture_width = rect[2] * 0.52;
        (
            [
                rect[0] + pad * 1.3,
                rect[1] + pad,
                (rect[2] - picture_width - pad * 2.3).max(page.scaled(120.0)),
                (rect[3] - pad * 2.0).max(0.0),
            ],
            [
                rect[0] + rect[2] - picture_width - pad,
                rect[1] + pad,
                picture_width,
                (rect[3] - pad * 2.0).max(0.0),
            ],
        )
    } else {
        let picture_height = rect[3] * 0.50;
        (
            [
                rect[0] + pad,
                rect[1] + pad + picture_height + gap,
                (rect[2] - pad * 2.0).max(0.0),
                (rect[3] - picture_height - gap - pad * 2.0).max(0.0),
            ],
            [
                rect[0] + pad,
                rect[1] + pad,
                (rect[2] - pad * 2.0).max(0.0),
                picture_height,
            ],
        )
    };

    let screenshot = row.screenshot.as_ref().and_then(|shot| {
        let arriving = store.art.fade(&shot.url, PICTURE_FADE);
        store.art.picture(&shot.url).map(|path| (path, arriving))
    });
    let fetched_icon = row
        .icon
        .is_none()
        .then(|| row.icon_url.clone())
        .flatten()
        .and_then(|url| {
            let arriving = store.art.fade(&url, PICTURE_FADE);
            store.art.picture(&url).map(|path| (path, arriving))
        });

    // Work out all type before borrowing the renderer. The developer sits on
    // its own line, like an App Store publisher link, rather than disappearing
    // into the summary.
    let eyebrow = page.line(Text::Caption);
    let title_line = page.line(Text::Display);
    let developer_line = page.line(Text::Caption);
    let summary_line = page.line(Text::Body);
    let developer = if row.developer.trim().is_empty() {
        String::new()
    } else {
        crate::message!("by-publisher", "publisher" => row.developer.trim().to_string())
    };
    let developer_room = if developer.is_empty() {
        0.0
    } else {
        developer_line
    };
    let fixed = eyebrow + title_line + developer_room + gap * 0.75;
    let summaries = (((words[3] - fixed) / summary_line).floor() as usize).clamp(1, 3);
    let wrapped = page.ui().wrap(
        Text::Body,
        &row.summary.replace(['\n', '\r'], " "),
        words[2],
    );
    let cut = wrapped.len() > summaries;
    let mut said: Vec<String> = wrapped.into_iter().take(summaries).collect();
    if cut && !said.is_empty() {
        let last = said.len() - 1;
        said[last] = shortened(page, Text::Body, &said[last], words[2]);
    }
    let named = one_line(page, Text::Display, &row.name, words[2]);
    let developer =
        (!developer.is_empty()).then(|| one_line(page, Text::Caption, &developer, words[2]));
    let stamp = stamp_of(row);
    let stamp_width = stamp
        .as_ref()
        .map(|(word, _)| page.measure(Text::Caption, word) + pad * 1.8)
        .unwrap_or(0.0);
    let eyebrow_min = page.scaled(60.0);

    let icons = page.icons();
    let ui = page.ui();
    ui.card(visual, Surface::Control, Role::Glass, 0.22 * fade);
    let screenshot_drawn = screenshot.as_ref().is_some_and(|(path, arriving)| {
        ui.picture(visual, radius, path, Fit::Cover, fade * arriving)
    });
    if !screenshot_drawn {
        let size = visual[2]
            .min(visual[3])
            .min(ui.s(if split { 148.0 } else { 120.0 }));
        let icon_at = [
            visual[0] + (visual[2] - size) * 0.5,
            visual[1] + (visual[3] - size) * 0.5,
            size,
            size,
        ];
        let icon_drawn = match (&row.icon, &fetched_icon) {
            (Some(path), _) => ui.picture(icon_at, radius, path, Fit::Contain, fade),
            (None, Some((path, arriving))) => {
                ui.picture(icon_at, radius, path, Fit::Contain, fade * arriving)
            }
            (None, None) => false,
        };
        if !icon_drawn {
            ui.icon_tinted(icon_at, row.glyph, icons, Role::Text, 0.86 * fade);
        }
    }

    let block = eyebrow
        + title_line
        + developer.as_ref().map(|_| developer_line).unwrap_or(0.0)
        + summary_line * said.len() as f32
        + gap * 0.75;
    let mut top = words[1] + (words[3] - block).max(0.0) * 0.5;
    let eyebrow_width = (words[2] - stamp_width - gap * 0.5).max(eyebrow_min);
    ui.label_tinted(
        [words[0], top, eyebrow_width, eyebrow],
        Text::Caption,
        crate::i18n::text("featured-on-flathub"),
        // The accent is deliberately dark in some shell themes and does not
        // carry enough contrast as small type over glass. The hero's light
        // and artwork already supply the accent; this line should stay easy
        // to read on every wallpaper.
        ui.tinted(Role::TextSoft, fade),
        Align::Left,
    );
    if let Some((word, role)) = &stamp {
        badge(
            ui,
            [
                words[0] + words[2] - stamp_width,
                top,
                stamp_width,
                eyebrow * 1.35,
            ],
            word,
            *role,
            fade,
        );
    }
    top += eyebrow + gap * 0.35;
    ui.label_tinted(
        [words[0], top, words[2], title_line],
        Text::Display,
        &named,
        ui.tinted(Role::Text, fade),
        Align::Left,
    );
    top += title_line;
    if let Some(developer) = developer {
        ui.label_tinted(
            [words[0], top, words[2], developer_line],
            Text::Caption,
            &developer,
            ui.tinted(Role::Text, fade * 0.78),
            Align::Left,
        );
        top += developer_line;
    }
    for line in said {
        ui.label_tinted(
            [words[0], top, words[2], summary_line],
            Text::Body,
            &line,
            ui.tinted(Role::TextSoft, fade),
            Align::Left,
        );
        top += summary_line;
    }
}

fn stamp_of(row: &Row) -> Option<(String, Role)> {
    match &row.kind {
        Kind::Repo { disabled: true, .. } => {
            Some((crate::i18n::text("off").into(), Role::TextSoft))
        }
        _ if row.updatable => Some((crate::i18n::text("update").into(), Role::Accent)),
        _ if row.installed => Some((crate::i18n::text("installed").into(), Role::TextSoft)),
        _ => None,
    }
}

/// A word in a pill: the shape a store says "installed" in.
///
/// Drawn as a chip under a label rather than as a bare right-aligned word,
/// because at the end of a row of prose a bare word reads as more prose.
pub fn badge(ui: &mut lxb_app::lxb_render::Ui, rect: [f32; 4], word: &str, role: Role, alpha: f32) {
    let strength = match role {
        Role::Accent => 0.30,
        _ => 0.12,
    };
    ui.chip(rect, ui.tinted(role, strength * alpha));
    // **The word is white, whatever the pill means.** Accent on accent is the
    // one combination in this language that cannot be read: an accented word
    // on a chip of its own colour is a word the eye has to hunt for, and
    // Update — the pill that most wants reading — was the one wearing it. What
    // a pill means is said by the chip behind it.
    ui.label_tinted(
        rect,
        Text::Caption,
        word,
        ui.tinted(Role::Text, alpha * 0.95),
        Align::Centre,
    );
}

/// As much of a string as fits on one line, with an ellipsis where it does not.
pub fn one_line(page: &mut Page, text: Text, string: &str, room: f32) -> String {
    let flat = string.replace(['\n', '\r'], " ");
    if page.measure(text, &flat) <= room {
        return flat;
    }
    let lines = page.ui().wrap(text, &flat, room);
    let first = lines.first().cloned().unwrap_or_default();
    shortened(page, text, &first, room)
}

/// One line, cut down until it fits, with an ellipsis to say it was cut.
///
/// Wrapping alone is not enough. A line breaks between words, so a single word
/// wider than the room it has comes back whole — "Repositories" in a panel too
/// narrow for it was drawn straight through the count beside it. Nothing here
/// clips, so the only way to make text fit is to make it shorter.
/// It always ends in one, because it is only asked for where something was
/// left out.
pub fn shortened(page: &mut Page, text: Text, line: &str, room: f32) -> String {
    let mut kept = line.trim_end().to_string();
    loop {
        let with = format!("{}…", kept.trim_end());
        if kept.is_empty() || page.measure(text, &with) <= room {
            return with;
        }
        kept.pop();
    }
}

/// What runs across the bottom of every screen: a job in flight, or what went
/// wrong last. Answers the top of the space that is left for everything else.
fn footer(store: &mut Store, page: &mut Page, room: [f32; 4], screen: &Screen) {
    let gap = page.metric(Metric::Gap);
    let caption = page.line(Text::Caption);
    let bottom = room[1] + room[3];

    if let Some(running) = &store.running {
        let step = if running.name.is_empty() {
            running.step.clone()
        } else {
            format!("{}  ·  {}", running.name, running.step)
        };
        let transferred = running.transferred;
        let of = crate::message!("place-of-total", "place" => running.at.max(1), "total" => running.of.max(1));
        let stopping = running.stopping;
        // The bar's own position, which follows the transaction rather than
        // stepping with it four times a second.
        let through = store.anim.through;
        let height = caption * 2.0 + gap;
        let top = bottom - height;
        let thick = page.scaled(6.0);

        let ui = page.ui();
        ui.label(
            [room[0], top, room[2] * 0.66, caption],
            Text::Caption,
            &step,
            if stopping { Role::TextSoft } else { Role::Text },
            Align::Left,
        );
        let said = if transferred > 0 {
            format!("{}  ·  {of}", flatpak::size(transferred))
        } else {
            of
        };
        ui.label(
            [room[0], top, room[2], caption],
            Text::Caption,
            &said,
            Role::TextSoft,
            Align::Right,
        );
        let bar = [room[0], top + caption + gap * 0.5, room[2], thick];
        ui.chip(bar, ui.tinted(Role::Glass, 0.5));
        if through > 0.0 {
            let run = bar[2] * through.clamp(0.0, 1.0);
            ui.chip([bar[0], bar[1], run, bar[3]], ui.role(Role::Accent));
            // The light a bar carries at its leading edge, which is what makes
            // it read as filling rather than as having been filled.
            ui.glow(
                [
                    bar[0] + run - thick * 3.0,
                    bar[1] - thick,
                    thick * 6.0,
                    thick * 3.0,
                ],
                Role::Accent,
                0.35,
            );
        }
        return;
    }

    // The legend takes the corner, the way the shell's start screen writes its
    // own buttons opposite the clock. What is left of that line is where
    // anything this store has to say goes, and the legend answers how much of
    // it there is: written the other way round, a long sentence would be laid
    // straight through the pictures of the buttons.
    let hints = hints_for(store, screen);
    let tall = crate::legend::height(page).max(caption);
    let top = bottom - tall;
    let pad = page.pad_in_hand();
    let left = crate::legend::row(page, room[0] + room[2], top + tall * 0.5, &hints, pad);

    // Ink, not the accent. `Role::Accent` is `#8B5CF6`, a violet, and a
    // sentence set in it on this page's own violet ground reads as a blue
    // smudge rather than as words — the same trap as a mark drawn in the
    // accent, which this page already knows about. What a line down here means
    // is said by which colour of ink it is: white for something that went
    // right, `Role::Danger` for something that did not, and soft grey for the
    // store talking about itself.
    let (said, role) = match (&store.trouble, &store.note) {
        (Some(trouble), _) => (trouble.clone(), Role::Danger),
        (None, Some(note)) => (note.clone(), Role::Text),
        (None, None) if store.rereading() => (
            crate::i18n::text("reading-the-machine").to_string(),
            Role::TextSoft,
        ),
        (None, None) => (String::new(), Role::TextSoft),
    };
    if !said.is_empty() {
        let width = (left - gap - room[0]).max(0.0);
        let said = one_line(page, Text::Caption, &said, width);
        page.ui().label(
            [room[0], top, width, tall],
            Text::Caption,
            &said,
            role,
            Align::Left,
        );
    }
}

/// What the buttons do, as a picture of each one and the word for what it
/// does.
///
/// Two pairs at the most, and every pair names a button that really does
/// something where the light is standing — the shell's own rule, and the
/// reason nothing here says anything about the arrows. Moving is the one thing
/// a page does not have to explain.
fn hints_for(store: &Store, screen: &Screen) -> Vec<crate::legend::Hint> {
    use crate::legend::{hint, Button};
    match (screen, store.column) {
        (Screen::Detail { id }, _) => crate::detail::hints(store, id),
        // Back is the whole of it while a job is running elsewhere: that page
        // has no controls left, and a legend naming a press names nothing.
        (Screen::Repository { name, scope }, _) => {
            let mut hints = Vec::new();
            if !store.repository_buttons(name, *scope).is_empty() {
                hints.push(hint(crate::i18n::text("press"), Button::Accept));
            }
            hints.push(hint(crate::i18n::text("back"), Button::Back));
            hints
        }
        // The address row is a control that is pressed, so what the press
        // does depends on whether it has been pressed yet.
        (Screen::AddRepository, _) if store.adding.typing => vec![
            hint(crate::i18n::text("add-it"), Button::Accept),
            hint(crate::i18n::text("stop-typing"), Button::Back),
        ],
        (Screen::AddRepository, _) if store.content == crate::flatpak::KNOWN.len() => {
            vec![
                hint(crate::i18n::text("type"), Button::Accept),
                hint(crate::i18n::text("back"), Button::Back),
            ]
        }
        (Screen::AddRepository, _) => {
            vec![
                hint(crate::i18n::text("add-it"), Button::Accept),
                hint(crate::i18n::text("back"), Button::Back),
            ]
        }
        // On the shelves there is nowhere further back to go, so Back names
        // nothing and is left off.
        (Screen::Browse, Column::Shelves) => {
            with_order(store, vec![hint(crate::i18n::text("open"), Button::Accept)])
        }
        // A field is typed into rather than pressed, so what is worth saying
        // about it is the way out.
        (Screen::Browse, Column::Field) => with_order(
            store,
            vec![hint(crate::i18n::text("shelves"), Button::Back)],
        ),
        (Screen::Browse, Column::Listing) => with_order(
            store,
            vec![
                // What the press really does on the row the light is on. A
                // runtime has no page to open — nothing in any catalogue
                // describes one — so a press on it fetches it, and a legend
                // saying Open there would name something that cannot happen.
                hint(
                    match store.chosen().map(|row| &row.kind) {
                        Some(Kind::Support { .. }) => crate::i18n::text("update"),
                        _ => crate::i18n::text("open"),
                    },
                    Button::Accept,
                ),
                hint(crate::i18n::text("shelves"), Button::Back),
            ],
        ),
    }
}

/// Put Sort into a legend, where this shelf has anything to sort.
///
/// Between the act and the way out, because it is neither: it is where the
/// other answers about the listing live. On a shelf with nothing on it — an
/// empty Search, most of all — it is left off, which is the rule the rest of
/// this row keeps: a legend naming a button that does nothing is worse than
/// naming none.
fn with_order(store: &Store, hints: Vec<crate::legend::Hint>) -> Vec<crate::legend::Hint> {
    use crate::legend::{hint, Button};
    if !store.can_be_ordered() {
        return hints;
    }
    let mut with = hints;
    let at = with
        .iter()
        .position(|one| one.on == Button::Back)
        .unwrap_or(with.len());
    with.insert(at, hint(crate::i18n::text("sort"), Button::Options));
    with
}

/// What a click landed on, if it landed on anything this store put there.
pub fn pressed(store: &mut Store, page: &mut Page) {
    if store.opening() {
        return;
    }
    // **A crossing takes no presses.** Both pages are on the screen while one
    // grows out of the other's card, and both write down where a pointer can
    // land — the page over the listing at whatever size it has reached, the
    // listing under it at its own. A click landing in the middle of that
    // would be answered by whichever of the two wrote its target down last,
    // which is not the one under the pointer. Directions are not blocked:
    // they go to the screen that has them, and that is never in doubt.
    if store.in_the_crossing() {
        return;
    }
    // A bar with a hand still on it. Answered before anything else, and
    // before it is asked whether the press that began the drag landed
    // anywhere: while a bar is held it owns the pointer, wherever the pointer
    // has since wandered to — off the bar, off the list, out of the window.
    let (on_the_list, at) = on_a_bar(page, LIST_BAR_SPOT);
    if on_the_list {
        if let Some(share) = at.and_then(|at| store.listing_pulled(at)) {
            store.pull_listing_to(share);
        }
        return;
    }
    let (on_the_content, at) = on_a_bar(page, CONTENT_BAR_SPOT);
    if on_the_content {
        if let Some(share) = at.and_then(|at| store.content_pulled(at)) {
            store.pull_content_to(share);
        }
        return;
    }
    // The legend is a row of controls as much as a row of words, and on these
    // pages it is the only place Back is drawn at all. Answered before
    // anything else: it is the last thing drawn on every page, so its targets
    // sit over whatever the page left underneath them.
    if let Some(button) = crate::legend::pressed(page, &hints_for(store, &store.screen)) {
        store.act(page, button.action());
        return;
    }
    match store.screen.clone() {
        Screen::Browse => {
            // Before the shelves and the rows, because it is the one control
            // that stands over the listing rather than in it.
            if page.pressed(FIELD_SPOT) {
                store.point_at_field();
                return;
            }
            for index in 0..store.shelves().len() {
                if page.pressed(SHELF_SPOT + index as u32) {
                    store.point_at_shelf(index);
                    return;
                }
            }
            for index in 0..store.rows.len() {
                if page.pressed(ROW_SPOT + index as u32) {
                    let was_here = store.column == Column::Listing && store.row == index;
                    let opens_on_click = store
                        .rows
                        .get(index)
                        .is_some_and(|row| row.kind.opens_on_click());
                    store.point_at_row(index);
                    if opens_on_click || was_here {
                        store.act(page, lxb_app::lxb_toolkit::input::Action::Accept);
                    }
                    return;
                }
            }
        }
        _ => crate::detail::pressed(store, page),
    }
}

/// A hand on a bar: whether one is there at all, and where the pointer has
/// taken it.
///
/// The press that began it is consumed whatever came of it, so a press on a
/// bar is never also read as a press on what is behind it. A finger taps one
/// every time it takes hold of a list by its edge, and a finger does not drag
/// bars — it drags the list.
fn on_a_bar(page: &mut Page, id: u32) -> (bool, Option<[f32; 2]>) {
    let fired = page.pressed(id);
    let at = page.dragging(id);
    (fired || at.is_some(), at)
}

/// Put the light in the pane a gesture was pointed at, before the directions
/// it turned into are acted on.
///
/// A wheel is directions like any other control, so nothing here moves
/// anything: the store acts on those directions itself, in the order they
/// arrived. What a wheel has that a key has not is somewhere it was pointed,
/// and this is that. The shelf panel and the application list are two lists,
/// and a wheel over one of them must not walk the other merely because that is
/// where the light was last left.
///
/// A gesture over neither of them — the heading, the space beside the legend —
/// still scrolls whichever list the light is in. A wheel that did nothing at
/// all is the fault this began as.
pub fn aim_at(store: &mut Store, scroll: lxb_app::Scroll) {
    if store.opening() {
        return;
    }

    if store.screen == Screen::Browse {
        if let Some(column) =
            browse_scroll_column(scroll.spot, store.rows.len(), store.shelves().len())
        {
            store.column = column;
        }
    } else if let Spot::Control(id) = scroll.spot {
        // The bar counts as the band it belongs to: a finger takes hold of a
        // list by its edge as readily as by its middle, and drags the list.
        if id == CONTENT_BAR_SPOT
            || id
                .checked_sub(CONTENT_SPOT)
                .is_some_and(|at| (at as usize) < store.content_depth())
        {
            store.band = Band::Content;
        }
    }
}

fn browse_scroll_column(spot: Spot, rows: usize, shelves: usize) -> Option<Column> {
    let Spot::Control(id) = spot else {
        return None;
    };
    let over_rows = id == LIST_SCROLL_SPOT
        || id == LIST_BAR_SPOT
        || id
            .checked_sub(ROW_SPOT)
            .is_some_and(|at| (at as usize) < rows);
    let over_shelves = id == SHELF_SCROLL_SPOT
        || id
            .checked_sub(SHELF_SPOT)
            .is_some_and(|at| (at as usize) < shelves);
    if over_rows {
        Some(Column::Listing)
    } else if over_shelves {
        Some(Column::Shelves)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The count each shelf says, in the session's language — asked of the
    /// catalog the way the corner asks for it, because which language this
    /// machine is in is not part of what the corner has to get right.
    fn says(id: &str, count: usize) -> String {
        let mut args = crate::i18n::FluentArgs::new();
        args.set("count", count);
        crate::i18n::format(id, &args)
    }

    #[test]
    fn a_count_in_the_corner_says_what_it_is_counting() {
        assert_eq!(
            counted(Shelf::Section(crate::catalogue::Section::Games), 773),
            says("count-apps", 773)
        );
        assert_eq!(
            counted(Shelf::Installed, 36),
            says("count-apps-installed", 36)
        );
        assert_eq!(
            counted(Shelf::Updates, 13),
            says("count-updates-waiting", 13)
        );
        assert_eq!(counted(Shelf::Search, 135), says("count-apps-found", 135));
        assert_eq!(
            counted(Shelf::Repositories, 2),
            says("count-repositories", 2)
        );
    }

    /// And in English and in Polish, which is what the line above cannot
    /// state. Polish has four forms of a noun where English has two, and the
    /// two counts that catch a rule written by hand are 22 and 112.
    #[test]
    fn a_count_is_written_the_way_each_language_writes_one() {
        let catalog = crate::i18n::Catalog::new(crate::i18n::RESOURCES);
        let said = |locale: &str, count: usize| {
            let mut args = crate::i18n::FluentArgs::new();
            args.set("count", count);
            catalog.format_for(locale, "count-apps", &args)
        };
        assert_eq!(said("en", 1), "1 application");
        assert_eq!(said("en", 773), "773 applications");
        assert_eq!(said("pl", 1), "1 aplikacja");
        assert_eq!(said("pl", 22), "22 aplikacje");
        assert_eq!(said("pl", 112), "112 aplikacji");
        assert_eq!(said("pl", 773), "773 aplikacje");
        // Russian draws the lines where Polish does: 22 is *few* in both and
        // 12 is *many* in both, which is why the rule is not "ends in 1".
        assert_eq!(said("ru", 1), "1 приложение");
        assert_eq!(said("ru", 22), "22 приложения");
        assert_eq!(said("ru", 112), "112 приложений");
        // German and Brazilian Portuguese have two forms, Chinese one.
        assert_eq!(said("de", 1), "1 Anwendung");
        assert_eq!(said("de", 773), "773 Anwendungen");
        assert_eq!(said("pt_BR", 1), "1 aplicativo");
        assert_eq!(said("pt_BR", 773), "773 aplicativos");
        assert_eq!(said("zh_CN", 1), "1 个应用程序");
        assert_eq!(said("zh_CN", 773), "773 个应用程序");
    }

    #[test]
    fn one_of_something_is_not_one_somethings() {
        let every = [
            Shelf::Search,
            Shelf::Updates,
            Shelf::Installed,
            Shelf::Repositories,
            Shelf::Section(crate::catalogue::Section::Games),
        ];
        for shelf in every {
            let said = counted(shelf, 1);
            assert!(said.contains('1'), "a count of one lost its number: {said}");
            // The form of the noun is the catalog's to choose, so what is
            // checked here is that it chose the singular one — whichever word
            // that is in this language.
            let plural = counted(shelf, 5);
            assert_ne!(
                said.replacen('1', "5", 1),
                plural,
                "a count of one was written as a plural: {said}"
            );
        }
    }

    #[test]
    fn a_shelf_leaves_its_panel_by_dimming_and_being_cut_without_a_blink() {
        // A hundred-point row, four hundred points of list, and a ramp of
        // forty: a row is at its faintest by the time it is that far out.
        let row = 100.0;
        let list = [0.0, 0.0, 100.0, 400.0];
        let ramp = 40.0;
        let at = |top: f32| showing_in([0.0, top, 100.0, row], list, ramp);

        assert_eq!(at(100.0), 1.0, "a row wholly inside the list was dimmed");
        assert_eq!(at(300.0), 1.0, "the last whole row was dimmed");

        // A row over an edge is faint, and how much of it there is to be faint
        // is what the cut decides.
        assert!(at(340.0) < 1.0, "a row over the edge was not dimmed at all");
        assert!(
            (at(340.0) - GHOST).abs() < 0.001,
            "a row a whole ramp out was {} rather than a ghost's ink",
            at(340.0)
        );
        assert!((at(-ramp) - GHOST).abs() < 0.001, "the same at the top");

        // Nothing is drawn for a row with none of itself in the list, and
        // everything with any of itself in it is.
        assert!(any_of_it_shows([0.0, 399.0, 100.0, row], list));
        assert!(!any_of_it_shows([0.0, 400.0, 100.0, row], list));
        assert!(any_of_it_shows([0.0, -row + 1.0, 100.0, row], list));
        assert!(!any_of_it_shows([0.0, -row, 100.0, row], list));

        // It only ever dims as it goes: no step, no blink, all the way out.
        let mut before = 1.0;
        for step in 0..60 {
            let now = at(300.0 + step as f32 * 4.0);
            assert!(
                now <= before + 0.001,
                "a row leaving grew brighter at {step}"
            );
            before = now;
        }
    }

    #[test]
    fn a_row_answers_to_the_pointer_over_what_of_it_is_really_there() {
        let list = [0.0, 100.0, 500.0, 400.0];
        let row = |top: f32| pressable([0.0, top, 500.0, 100.0], list);

        assert_eq!(row(200.0), Some([0.0, 200.0, 500.0, 100.0]), "a whole row");
        // Three-quarters of a row hanging over the top: what answers is the
        // three-quarters that is in the list, not the whole of it.
        assert_eq!(row(75.0), Some([0.0, 100.0, 500.0, 75.0]));
        // A slice is a cue, not a control.
        assert_eq!(row(50.0), None, "a row barely in the list took a press");
        assert_eq!(row(450.0), None, "the same at the foot");
    }

    #[test]
    fn soft_edges_follow_real_overflow_and_clear_at_each_end() {
        let content = 1_000.0;
        let viewport = 400.0;
        let band = 80.0;

        assert_eq!(
            edge_strengths(content, -20.0, viewport, band),
            [0.0, 1.0],
            "the beginning invented content above itself"
        );
        assert_eq!(
            edge_strengths(content, 300.0, viewport, band),
            [1.0, 1.0],
            "a middle page did not soften both continuing edges"
        );
        assert_eq!(
            edge_strengths(content, 600.0, viewport, band),
            [1.0, 0.0],
            "the end still implied content below itself"
        );
        assert_eq!(edge_strengths(300.0, 0.0, viewport, band), [0.0, 0.0]);

        let mut before = 0.0;
        for hidden in (0..=80).step_by(4) {
            let now = edge_strengths(content, hidden as f32, content - viewport, band)[0];
            assert!(now >= before, "the top edge became clearer while leaving");
            before = now;
        }
    }

    #[test]
    fn a_wheel_targets_the_pane_under_the_pointer_including_its_gaps() {
        assert_eq!(
            browse_scroll_column(Spot::Control(LIST_SCROLL_SPOT), 20, 15),
            Some(Column::Listing)
        );
        assert_eq!(
            browse_scroll_column(Spot::Control(ROW_SPOT + 12), 20, 15),
            Some(Column::Listing)
        );
        assert_eq!(
            browse_scroll_column(Spot::Control(SHELF_SCROLL_SPOT), 20, 15),
            Some(Column::Shelves)
        );
        assert_eq!(
            browse_scroll_column(Spot::Control(SHELF_SPOT + 8), 20, 15),
            Some(Column::Shelves)
        );
        assert_eq!(browse_scroll_column(Spot::Nothing, 20, 15), None);
        assert_eq!(
            browse_scroll_column(Spot::Control(BUTTON_SPOT), 20, 15),
            None
        );
    }

    /// Home's own shape at a window this store is often in: a promoted
    /// application three cards tall, then four sections of a heading and three
    /// lines of cards.
    fn home_lines() -> Vec<Line> {
        let mut lines = vec![Line {
            kind: LineKind::Hero,
            rows: vec![0],
        }];
        let mut row = 1;
        for _ in 0..4 {
            lines.push(Line {
                kind: LineKind::Heading,
                rows: vec![row],
            });
            row += 1;
            for _ in 0..3 {
                lines.push(Line {
                    kind: LineKind::Cells,
                    rows: vec![row, row + 1, row + 2],
                });
                row += 3;
            }
        }
        lines
    }

    /// One frame of the listing: lay the lines out, measure how many fit, and
    /// tell the store what it came out as — which is all `plan_the_listing`
    /// does before it draws anything.
    fn one_frame(store: &mut Store, tall: &Heights, viewport: f32, peek: f32) {
        let mut tops = Vec::new();
        let mut at = 0.0;
        for line in &store.lines {
            tops.push(at);
            at += tall.of(line.kind)
                + if line.kind == LineKind::Heading {
                    0.0
                } else {
                    tall.gap
                };
        }
        let room = fitting_lines(store, tall, viewport - peek);
        store.shape_is(Shape {
            columns: 3,
            room,
            tops,
            peek,
            viewport,
            deep: at,
        });
    }

    #[test]
    fn a_card_reached_down_home_settles_instead_of_rocking_under_itself() {
        let tall = Heights {
            hero: 243.0,
            heading: 58.0,
            wide: 79.0,
            card: 95.0,
            gap: 8.0,
        };
        let viewport = 700.0;
        let peek = 43.0;
        let mut store = Store::new();
        store.columns = 3;
        store.lines = home_lines();

        // Down the page a line at a time, standing on each one for four frames
        // — long enough for a listing that is going to rock to have rocked.
        // How many lines fit depends on which lines they are, so the room a
        // frame reports changes as the top moves; nothing the light does may
        // depend on that.
        let mut before = 0;
        for line in 0..store.lines.len() {
            if store.lines[line].kind == LineKind::Heading {
                continue;
            }
            store.row = store.lines[line].rows[0];
            let mut settled = Vec::new();
            for _ in 0..4 {
                one_frame(&mut store, &tall, viewport, peek);
                settled.push(store.top);
            }
            assert!(
                settled.windows(2).all(|two| two[0] == two[1]),
                "line {line} left the listing rocking between {settled:?}"
            );
            assert!(
                store.top >= before,
                "walking down line {line} scrolled the listing back up"
            );
            assert!(
                store.top <= line,
                "line {line} was above the top of its own page"
            );
            before = store.top;

            // And the card the light is on is whole on the page, which is what
            // all the settling is for.
            let top = store.line_tops[store.top] - peek;
            let foot = store.line_tops.get(line + 1).copied().unwrap_or(store.deep);
            assert!(
                store.line_tops[line] >= top && foot <= top + viewport,
                "line {line} was not whole on a page scrolled to {top}"
            );
        }
    }
}
