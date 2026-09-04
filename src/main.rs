//! DistriBumpy — a Flatpak store in the LineXinBar design language.
//!
//! It appears as **Software Hub**, on the shell and on any other desktop, and
//! it is an ordinary Wayland application: no shell protocol, nothing private,
//! and it runs under GNOME or Plasma as readily as under LineXinBar.
//!
//! Four threads, and only one of them draws:
//!
//! * the frame loop, which belongs to `lxb-app` and never blocks;
//! * a reader, which takes a second and a half over Flathub's catalogue;
//! * the worker, which runs one flatpak transaction at a time;
//! * a fetcher, for the screenshots that are the only thing not already here.

mod art;
mod catalogue;
mod detail;
mod draw;
mod flathub;
mod flatpak;
mod legend;
mod motion;
mod ratings;
mod sandbox;
mod store;

fn main() -> Result<(), String> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.iter().any(|one| one == "--version") {
        println!("distribumpy {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if arguments.iter().any(|one| one == "--help") {
        println!("{HELP}");
        return Ok(());
    }
    if let Some(at) = arguments.iter().position(|one| one == "--shot") {
        let path = arguments
            .get(at + 1)
            .ok_or("--shot needs a file to write")?;
        return shot(path, &arguments);
    }

    let mut store = store::Store::new();
    lxb_app::App::new("distribumpy", "Software Hub")
        .driven()
        .plain()
        .run(move |page| frame(&mut store, page))
}

fn frame(store: &mut store::Store, page: &mut lxb_app::Page) {
    let seconds = page.seconds();
    store.advance();
    store.animate(seconds);
    store.take_typing(page);
    store.take_answer(page);
    store.take_choice(page);
    // A pointer can have opened a dialog at the end of the previous frame.
    // Its flag has served its purpose; events arriving while the dialog is up
    // are intercepted by the toolkit itself.
    let _ = store.take_opened_modal();

    let actions: Vec<_> = page.actions().collect();
    // A wheel arrives as directions like any other control, and its gesture is
    // said again beside them with the one thing a direction cannot carry:
    // where the pointer was. The two are walked together, in the one order
    // they happened, so a gesture cannot overtake a press that came first —
    // and a wheel belongs to the pane under the hand rather than to whichever
    // column a keyboard last left the light in.
    let scrolls: Vec<_> = page.scrolls().collect();
    let mut opened_modal = false;
    for (index, action) in actions.into_iter().enumerate() {
        if let Some(scroll) = scrolls.iter().find(|scroll| scroll.covers(index)) {
            draw::aim_at(store, *scroll);
        }
        store.act(page, action);
        if store.take_opened_modal() {
            // Actions already queued for this redraw did not pass through the
            // dialog, because it did not exist when their events arrived.
            // Nothing behind a newly opened confirmation should move.
            opened_modal = true;
            break;
        }
    }

    draw::draw(store, page);
    if !opened_modal {
        draw::pressed(store, page);
    }
}

/// A picture of a page, with no display at all.
///
/// The same page function and the same renderer as the window, which is what
/// makes it worth looking at. It reads the real machine, so what it shows is
/// what is really on offer here.
fn shot(path: &str, arguments: &[String]) -> Result<(), String> {
    let named = |flag: &str| -> Option<String> {
        arguments
            .iter()
            .position(|one| one == flag)
            .and_then(|at| arguments.get(at + 1))
            .cloned()
    };
    let shelf = named("--shelf")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    // `None` rather than nought when it was not asked for: the light rests on
    // the sidebar until a row is named, and **the hero is row nought**, which
    // a number alone cannot tell apart from not having been named at all.
    let row = named("--row").and_then(|value| value.parse().ok());
    let app = named("--app");
    // A picture of the crossing rather than of a page at rest: how long
    // after the press, and whether the press is Back. See `--after` in HELP.
    let after: Option<f32> = named("--after").and_then(|value| value.parse().ok());
    let back = arguments.iter().any(|one| one == "--back");
    let width = named("--width")
        .and_then(|value| value.parse().ok())
        .unwrap_or(1600);
    let height = named("--height")
        .and_then(|value| value.parse().ok())
        .unwrap_or(900);

    let mut store = store::Store::ready();
    store.look_at(
        shelf,
        row,
        app.as_deref(),
        &named("--query").unwrap_or_default(),
    );
    if let Some(tab) = named("--tab").and_then(|value| value.parse().ok()) {
        store.look_at_tab(tab);
    }
    if let Some(name) = named("--repo") {
        store.look_at_repository(&name);
    }
    if arguments.iter().any(|one| one == "--add-repo") {
        store.look_at_adding();
    }

    // The first frame is what works out how many rows there is room for, and
    // where a list is scrolled to follows from that. `App::shot` draws twice
    // at one instant, so between the two the store is put where it is going.
    let mut drawn = 0;
    lxb_app::App::new("distribumpy", "Software Hub")
        .driven()
        .plain()
        .shot(path, width, height, 6.0, move |page| {
            frame(&mut store, page);
            drawn += 1;
            if drawn == 1 {
                store.settle_for_a_picture();
                if let Some(after) = after {
                    press_and_wind(&mut store, page, after, back);
                }
            }
        })
}

/// Open the page the light is on and count frames, for a picture of the
/// crossing between the two.
///
/// **The press has to be a real one.** What a page grows out of is where
/// drawing put the card that opened it, and only a frame that has been drawn
/// knows that — a store put on a detail page by hand has no card behind it
/// and nothing to grow out of.
fn press_and_wind(store: &mut store::Store, page: &mut lxb_app::Page, after: f32, back: bool) {
    use lxb_app::lxb_toolkit::input::Action;
    store.act(page, Action::Accept);
    if back {
        // All the way out first, so what is counted is the way back in.
        store.settle_for_a_picture();
        store.act(page, Action::Back);
    }
    let mut at = page.seconds();
    let mut gone = 0.0;
    while gone < after {
        at += FRAME;
        gone += FRAME;
        store.animate(at);
    }
}

/// One frame of the clock, at the sixty a second a window is drawn at.
const FRAME: f32 = 1.0 / 60.0;

const HELP: &str = "\
usage: distribumpy [--shot FILE [--shelf N] [--row N] [--app ID] [--tab N]
                    [--query TEXT] [--repo NAME] [--add-repo]
                    [--width N] [--height N]]

With no arguments it opens a window.

--shot writes a settled frame to a PNG with no display, through the same page
function and renderer the window uses, reading the machine it is run on. Every
animation is put where it is going first, so what it photographs is the page
at rest.

  --shelf N    which shelf, counting from Home at nought
  --row N      which row of the listing
  --app ID     open that application's page
  --tab N      which tab of it: 0 About, then whichever it offers
  --query TEXT what to have typed into the search field
  --repo NAME  open that repository's page
  --add-repo   open the page a repository is added from
  --after SEC  press the row the light is on and count that many seconds,
               for a picture of the page on its way out of its card rather
               than of a page at rest. Wants --row, not --app: a page has to
               be opened by a press to have a card to grow out of.
  --back       with --after, press Back instead, for the way back in

--version  print the version
--help     print this message";
