//! Everything on this page that is on its way somewhere.
//!
//! The store keeps one of these and advances it once a frame. Nothing here
//! decides anything and nothing here draws: it turns "the shelf changed" into
//! "these rows are 40% of the way in", so that drawing can ask where something
//! is rather than working it out.
//!
//! Two rules from the design language run through all of it. Easing is never
//! linear — every timer here goes through `motion::ease` or a spring before it
//! reaches a rectangle. And nothing vanishes before its transition ends, which
//! is why a screen being left is still drawn while it leaves.

use lxb_app::lxb_render::{Press, Pressing};
use lxb_app::lxb_toolkit::motion;

/// How long a listing takes to come in, and how far apart its rows start.
///
/// Shorter than the toolkit's own stagger, because a listing is twelve rows
/// rather than five tiles: at the toolkit's 45 ms the last row would still be
/// arriving half a second after the shelf was pressed.
const ROW_STAGGER: f32 = 0.022;
const ROW_SLIDE: f32 = motion::duration::ENTRY_SLIDE;

/// How long a page takes to grow out of the card that opened it.
///
/// The shell's own launch duration, because it is the shell's own gesture: a
/// tile pressed on the start screen becomes the application, and a card
/// pressed here becomes the page about it.
const GROW: f32 = motion::duration::LAUNCH_OPEN;

/// How much sooner the page over the listing is solid than it is grown.
///
/// Three, which puts it at full strength a little under halfway along. See
/// [`Anim::arriving`].
const SOLID: f32 = 3.0;

/// How far a row starts to the right of where it belongs, before scaling.
///
/// Small on purpose. A row starts at nothing and comes in as it fades, so what
/// this really sets is how much of a lean there is at half opacity.
const ROW_LEAN: f32 = 14.0;

/// When a list is near enough where it is going to stop being carried there.
///
/// A spring never quite arrives. Half a point is well under a pixel on any
/// display this runs on, and twenty points a second is a third of a pixel a
/// frame; stopping there is invisible, and it is what lets a still frame be
/// still rather than being redrawn for ever a thousandth of a point at a time.
const ARRIVED: f32 = 0.5;
const STILL: f64 = 20.0;

/// How firmly a list follows the row that is chosen.
///
/// The toolkit's card spring rather than its highlight spring: a list is a
/// heavy thing being carried, and the light crossing it is not.
const SCROLL_SPRING: f64 = motion::CARD_SPRING;

/// Everything in flight.
pub struct Anim {
    /// The clock the page was drawn at last, so that a step can be worked out
    /// from the one it is drawn at now.
    at: Option<f32>,
    /// How long since the last frame, clamped: a window that was not drawn for
    /// a second must not make everything jump a second's worth.
    pub dt: f32,

    /// Where the listing is scrolled to, in rows. Springs to whichever row is
    /// meant to be at the top.
    pub scroll: f32,
    speed: f64,

    /// Where the shelves are scrolled to, in points.
    ///
    /// Its own spring rather than the listing's, because the two are carried
    /// by different things: the listing follows the card the light is on, and
    /// the panel follows the shelf.
    pub shelves: Carried,

    /// The light that crosses the browsing page, which this store carries
    /// itself rather than leaving to the toolkit. See [`Light`].
    pub light: Light,

    /// How long the listing on screen has been the listing on screen.
    pub listed: f32,
    /// How far the page over the listing has grown out of the card that
    /// opened it: nought on the card, one filling the page. Raw, and read
    /// through [`Anim::grown`], which eases it.
    ///
    /// **One number, both ways.** Going back is the way in run backwards, so
    /// it is this number counted the other way rather than a second one of
    /// its own: a page half open when Back is pressed shrinks from where it
    /// really is, and one opened again halfway out grows from there.
    grown: f32,
    /// Which way that number is going.
    growing: bool,
    /// How long the store has been reading, which is the one wait with nothing
    /// at all behind it.
    pub reading: f32,

    /// The progress bar's own position, which follows the transaction rather
    /// than jumping to it four times a second.
    pub through: f32,

    /// Which screenshot is being crossed to, and how far across.
    pub crossing: f32,

    /// The press, which in a driven page is nobody else's job to run.
    ///
    /// The toolkit fires its own press animation when a control it numbered is
    /// clicked. A page that draws its own controls and moves its own light is
    /// never told, so a press on a controller would show nothing at all unless
    /// the page runs one itself.
    pressing: Pressing,
}

impl Default for Anim {
    fn default() -> Self {
        Self {
            at: None,
            dt: 1.0 / 60.0,
            scroll: 0.0,
            speed: 0.0,
            shelves: Carried::default(),
            light: Light::default(),
            listed: SETTLED,
            grown: 0.0,
            growing: false,
            reading: 0.0,
            through: 0.0,
            crossing: 1.0,
            pressing: Pressing::default(),
        }
    }
}

/// Long enough ago that everything timed from it has finished.
const SETTLED: f32 = 10.0;

impl Anim {
    /// Take one step, from the clock the page is being drawn at.
    pub fn advance(&mut self, seconds: f32, top: f32, reading: bool, through: f32) {
        let dt = match self.at {
            Some(before) => (seconds - before).clamp(0.0, 0.1),
            None => 1.0 / 60.0,
        };
        self.at = Some(seconds);
        self.dt = dt;

        self.listed += dt;
        let step = dt / GROW;
        self.grown = if self.growing {
            (self.grown + step).min(1.0)
        } else {
            (self.grown - step).max(0.0)
        };
        self.crossing = (self.crossing + dt / motion::duration::COLOUR_FADE).min(1.0);
        self.reading = if reading { self.reading + dt } else { 0.0 };

        let (at, speed) = motion::spring(
            self.scroll as f64,
            self.speed,
            top as f64,
            SCROLL_SPRING,
            dt as f64,
        );
        self.scroll = at as f32;
        self.speed = speed;
        // Near enough is where a list stops being carried; see ARRIVED.
        if (self.scroll - top).abs() < ARRIVED && self.speed.abs() < STILL {
            self.scroll = top;
            self.speed = 0.0;
        }

        // The bar catches up rather than stepping: a transaction reports four
        // times a second, and a bar that moved four times a second would look
        // like something going wrong rather than something going.
        self.through = motion::approach(self.through, through.clamp(0.0, 1.0), dt * 1.6);

        self.pressing.advance(dt);
    }

    /// The listing changed: bring it in again.
    pub fn listed_anew(&mut self) {
        self.listed = 0.0;
    }

    /// A card was pressed: grow the page it opens out of it.
    ///
    /// Never from nought, always from wherever the last crossing left off.
    /// See [`Anim::grown`].
    pub fn opened(&mut self) {
        self.growing = true;
    }

    /// Back was pressed: shrink the page over the listing into its card.
    pub fn closed(&mut self) {
        self.growing = false;
    }

    /// A different screenshot was chosen.
    pub fn crossed(&mut self) {
        self.crossing = 0.0;
    }

    /// A job started, so the bar starts from nothing rather than from wherever
    /// the last one finished.
    pub fn restarted(&mut self) {
        self.through = 0.0;
    }

    /// Somebody pressed something.
    pub fn pressed(&mut self) {
        self.pressing.press();
    }

    /// The press to hand a control, which is what dips it and brings it back.
    pub fn press(&self, lit: bool) -> Press {
        self.pressing.state(lit)
    }

    /// Put a list where it is going without carrying it there.
    ///
    /// Used where there will be no second frame to carry it in: `App::shot`
    /// draws twice at one instant, so a page photographed mid-animation would
    /// be a photograph of the first frame of it. A picture of a page should be
    /// a picture of the page at rest.
    pub fn settle(&mut self, top: f32) {
        self.scroll = top;
        self.speed = 0.0;
        self.shelves.settle(self.shelves.wanted);
        self.light.settle();
        self.listed = SETTLED;
        self.grown = if self.growing { 1.0 } else { 0.0 };
        self.crossing = 1.0;
    }

    /// How far the page over the listing has grown out of its card, nought
    /// to one, eased.
    ///
    /// Everything about the crossing reads this and nothing else: the
    /// rectangle the page is drawn in, how far the listing behind it has
    /// stepped back and faded, and how solid the page itself is. **Two clocks
    /// is the defect this avoids** — a spring beside an ease lands a page and
    /// its own chrome on different frames.
    pub fn grown(&self) -> f32 {
        motion::ease(self.grown)
    }

    /// How solid the page over the listing is, nought to one.
    ///
    /// **Not the same number as how far it has grown**, and the difference is
    /// what makes the crossing readable. There is nothing to hide the listing
    /// behind the page with — a sheet of glass over it refracts the whole
    /// page whatever its tint says — so the page becomes solid well before it
    /// finishes growing, and spends most of the crossing as one page over a
    /// ghost rather than as two pages at half strength each.
    ///
    /// Nought where it is nought and one where it is one, so both ends of the
    /// crossing still land exactly on the page they hand over to.
    pub fn arriving(&self) -> f32 {
        (self.grown() * SOLID).min(1.0)
    }

    /// Whether the page over the listing is all the way back in its card, so
    /// that a page being left can be let go of.
    pub fn on_the_card(&self) -> bool {
        !self.growing && self.grown <= 0.0
    }

    /// Whether the page over the listing fills it, which is when the listing
    /// under it is no longer worth drawing.
    pub fn filling_the_page(&self) -> bool {
        self.growing && self.grown >= 1.0
    }

    /// Where one row of a listing is, given how far down the listing it is.
    ///
    /// Answers how far in it is and how far right of where it belongs it still
    /// is. Rows arrive one after another rather than together, which is what
    /// makes a list read as a list rather than as a slab appearing.
    pub fn row_in(&self, offset: usize, scale: f32) -> Coming {
        let started = self.listed - offset as f32 * ROW_STAGGER;
        let through = motion::ease((started / ROW_SLIDE).clamp(0.0, 1.0));
        Coming {
            fade: through,
            lean: (1.0 - through) * ROW_LEAN * scale,
        }
    }

    /// The mark on the opening screen, which has nothing to say but that it is
    /// working.
    pub fn breath(&self) -> f32 {
        0.94 + 0.06 * motion::pulse(self.reading)
    }

    /// How far across an indeterminate bar's traveller is, nought to one.
    ///
    /// It goes across, and comes back, on the same easing everything else
    /// uses, so it never looks like a bar that knows how far it has to go.
    pub fn sweep(&self) -> f32 {
        let cycle = motion::duration::PULSE * 1.6;
        let round = (self.reading % (cycle * 2.0)) / cycle;
        if round < 1.0 {
            motion::ease(round)
        } else {
            1.0 - motion::ease(round - 1.0)
        }
    }
}

/// How far in something is, and how far it still has to come.
#[derive(Debug, Clone, Copy)]
pub struct Coming {
    /// Nought to one. Multiply an opacity by it.
    pub fade: f32,
    /// Points still to the right of where it belongs. Add it to an x.
    pub lean: f32,
}

/// One rectangle part of the way to another.
///
/// Both corners together rather than a corner and a size, so a rectangle
/// growing out of a small one lands on the far one exactly: eased separately,
/// a width and a left edge arrive at different pixels.
pub fn between(from: [f32; 4], to: [f32; 4], through: f32) -> [f32; 4] {
    let at = |a: f32, b: f32| a + (b - a) * through;
    let left = at(from[0], to[0]);
    let top = at(from[1], to[1]);
    [
        left,
        top,
        (at(from[0] + from[2], to[0] + to[2]) - left).max(0.0),
        (at(from[1] + from[3], to[1] + to[3]) - top).max(0.0),
    ]
}

/// How faint something has to be before it is not worth drawing at all.
///
/// A card at a hundredth of an opacity is a card nobody can see and a card the
/// renderer would still lay out, measure and shape every word of.
pub const WORTH_DRAWING: f32 = 0.01;

impl Coming {
    /// Move a rectangle to where this is now.
    pub fn moved(self, rect: [f32; 4]) -> [f32; 4] {
        [rect[0] + self.lean, rect[1], rect[2], rect[3]]
    }
}

/// Something being carried to where it belongs, on the same spring a listing
/// is carried on.
///
/// Its own type because there are now two of these — the listing and the panel
/// of shelves beside it — and a second copy of a spring is a second chance to
/// tune one of them and not the other.
#[derive(Debug, Default, Clone, Copy)]
pub struct Carried {
    at: f32,
    speed: f64,
    /// Where it was last told to go, so that settling for a photograph has
    /// somewhere to settle to.
    wanted: f32,
}

impl Carried {
    /// Where it is now.
    pub fn at(&self) -> f32 {
        self.at
    }

    /// Carry it one frame's worth towards where it belongs.
    pub fn advance(&mut self, to: f32, dt: f32) {
        self.wanted = to;
        let (at, speed) = motion::spring(
            self.at as f64,
            self.speed,
            to as f64,
            SCROLL_SPRING,
            dt as f64,
        );
        self.at = at as f32;
        self.speed = speed;
        // Near enough is where a list stops being carried; see ARRIVED.
        if (self.at - to).abs() < ARRIVED && self.speed.abs() < STILL {
            self.at = to;
            self.speed = 0.0;
        }
    }

    /// Put it where it is going without carrying it there.
    pub fn settle(&mut self, to: f32) {
        self.at = to;
        self.wanted = to;
        self.speed = 0.0;
    }
}

/// The light that crosses a page, carried here rather than by the toolkit.
///
/// `Page::glide` draws its own selection as a **capsule** — a radius of half
/// its height — which is right for a row and wrong for a card. A card is
/// `Metric::CardRadius` round; a capsule laid over one leaves the card's four
/// corners sticking out behind it, which is what a highlight must never do.
/// So the spring is the toolkit's own `HIGHLIGHT_SPRING` and the drawing is
/// `Ui::lit`, which unlike `Ui::selection` is told what shape to be.
#[derive(Debug, Default, Clone, Copy)]
pub struct Light {
    at: Option<[f32; 4]>,
    speed: [f32; 4],
    /// Put it where it is going on the next frame rather than carrying it
    /// there — what a photograph needs, and what a page arrived at fresh
    /// needs, because a light that flew in from the last page would be a light
    /// crossing a page nobody was looking at.
    snap: bool,
}

impl Light {
    /// Carry it a frame's worth towards a rectangle, and answer where it is.
    pub fn glide(&mut self, target: [f32; 4], dt: f32) -> [f32; 4] {
        let from = match self.at {
            Some(from) if !self.snap => from,
            _ => {
                self.snap = false;
                self.speed = [0.0; 4];
                self.at = Some(target);
                return target;
            }
        };
        let mut next = [0.0; 4];
        for (index, slot) in next.iter_mut().enumerate() {
            let (at, speed) = motion::spring(
                from[index] as f64,
                self.speed[index] as f64,
                target[index] as f64,
                motion::HIGHLIGHT_SPRING,
                dt as f64,
            );
            *slot = at as f32;
            self.speed[index] = speed as f32;
        }
        self.at = Some(next);
        next
    }

    /// Have the next frame put it where it belongs rather than carry it.
    pub fn settle(&mut self) {
        self.snap = true;
        self.speed = [0.0; 4];
    }
}

/// The largest rectangle of a given shape that fits inside another one,
/// centred in it.
///
/// This is what puts a screenshot in a frame of its own proportions instead of
/// in whatever rectangle was left over. AppStream declares every picture's
/// width and height, so the frame can be cut to the picture before the picture
/// has been fetched — which is why a frame never changes shape underneath a
/// picture arriving in it.
pub fn framed(room: [f32; 4], aspect: f32) -> [f32; 4] {
    if !aspect.is_finite() || aspect <= 0.0 || room[2] <= 0.0 || room[3] <= 0.0 {
        return room;
    }
    let (width, height) = if room[2] / room[3] > aspect {
        (room[3] * aspect, room[3])
    } else {
        (room[2], room[2] / aspect)
    };
    [
        room[0] + (room[2] - width) * 0.5,
        room[1] + (room[3] - height) * 0.5,
        width,
        height,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_is_cut_to_the_picture_and_never_out_of_the_room_it_has() {
        // A wide picture in a square box: as wide as the box, and shorter.
        let frame = framed([0.0, 0.0, 100.0, 100.0], 2.0);
        assert_eq!(frame, [0.0, 25.0, 100.0, 50.0], "a 2:1 picture in a square");

        // A tall picture in a wide box: as tall as the box, and narrower.
        let frame = framed([0.0, 0.0, 200.0, 100.0], 0.5);
        assert_eq!(
            frame,
            [75.0, 0.0, 50.0, 100.0],
            "a 1:2 picture in a 2:1 box"
        );

        // Exactly the same shape: exactly the room it has.
        assert_eq!(
            framed([10.0, 20.0, 160.0, 90.0], 16.0 / 9.0),
            [10.0, 20.0, 160.0, 90.0],
            "a picture already the right shape was moved"
        );
    }

    #[test]
    fn a_frame_with_nothing_known_about_it_keeps_the_room_it_was_given() {
        let room = [4.0, 5.0, 60.0, 40.0];
        assert_eq!(
            framed(room, 0.0),
            room,
            "a picture of no shape emptied a box"
        );
        assert_eq!(framed(room, f32::NAN), room);
        assert_eq!(
            framed([0.0, 0.0, 0.0, 40.0], 1.5),
            [0.0, 0.0, 0.0, 40.0],
            "a box with no width was divided by"
        );
    }

    #[test]
    fn a_list_follows_the_row_that_was_chosen_and_then_stops() {
        let mut anim = Anim::default();
        anim.advance(0.0, 0.0, false, 0.0);
        assert_eq!(anim.scroll, 0.0);

        // Eight rows of sixty points down, and half a second to get there.
        let mut at = 0.0;
        for _ in 0..30 {
            at += 1.0 / 60.0;
            anim.advance(at, 480.0, false, 0.0);
        }
        assert_eq!(
            anim.scroll, 480.0,
            "a list that had arrived was still being carried, which would \
             animate for ever and never let a frame be still"
        );
    }

    #[test]
    fn a_list_is_somewhere_between_where_it_was_and_where_it_is_going() {
        let mut anim = Anim::default();
        anim.advance(0.0, 0.0, false, 0.0);
        anim.advance(1.0 / 60.0, 300.0, false, 0.0);
        assert!(
            anim.scroll > 0.0 && anim.scroll < 300.0,
            "a list jumped rather than travelled: {}",
            anim.scroll
        );
    }

    #[test]
    fn a_row_further_down_a_listing_arrives_later_than_one_above_it() {
        let mut anim = Anim::default();
        anim.listed_anew();
        anim.advance(0.0, 0.0, false, 0.0);
        anim.advance(0.06, 0.0, false, 0.0);

        let first = anim.row_in(0, 1.0);
        let sixth = anim.row_in(5, 1.0);
        assert!(
            first.fade > sixth.fade,
            "every row arrived at once: {} against {}",
            first.fade,
            sixth.fade
        );
        assert!(
            sixth.lean > first.lean,
            "a row still coming was not still leaning"
        );
        assert!(first.fade < 1.0, "a listing was in before it began");
    }

    #[test]
    fn a_settled_listing_is_all_the_way_in_and_leaning_nowhere() {
        let mut anim = Anim::default();
        anim.listed_anew();
        anim.settle(180.0);
        assert_eq!(anim.scroll, 180.0);
        for offset in [0, 5, 40] {
            let coming = anim.row_in(offset, 1.0);
            assert_eq!(coming.fade, 1.0, "row {offset} was still arriving");
            assert_eq!(coming.lean, 0.0, "row {offset} was still leaning");
        }
        anim.opened();
        anim.settle(180.0);
        assert_eq!(anim.grown(), 1.0, "the page itself was still arriving");
    }

    /// One frame's worth of the clock, at the sixty a second everything here
    /// is drawn at.
    const FRAME: f32 = 1.0 / 60.0;

    fn wound(anim: &mut Anim, seconds: f32) {
        let mut at = anim.at.unwrap_or(0.0);
        let until = at + seconds;
        while at < until {
            at += FRAME;
            anim.advance(at, 0.0, false, 0.0);
        }
    }

    #[test]
    fn a_page_grows_out_of_its_card_and_shrinks_back_into_it() {
        let mut anim = Anim::default();
        anim.advance(0.0, 0.0, false, 0.0);
        assert!(anim.on_the_card(), "a listing began with a page over it");

        anim.opened();
        wound(&mut anim, GROW * 0.5);
        let halfway = anim.grown();
        assert!(
            halfway > 0.0 && halfway < 1.0,
            "a page was not on its way anywhere halfway through: {halfway}"
        );
        assert!(!anim.filling_the_page(), "a page arrived in half the time");
        wound(&mut anim, GROW);
        assert_eq!(anim.grown(), 1.0, "a page never finished arriving");
        assert!(anim.filling_the_page());

        anim.closed();
        wound(&mut anim, GROW * 0.5);
        assert!(
            anim.grown() > 0.0 && anim.grown() < 1.0,
            "a page left in one frame"
        );
        assert!(!anim.on_the_card(), "a page was let go of before it landed");
        wound(&mut anim, GROW);
        assert_eq!(anim.grown(), 0.0, "a page never finished leaving");
        assert!(anim.on_the_card());
    }

    #[test]
    fn both_ends_of_a_crossing_land_on_the_page_they_hand_over_to() {
        // The last frame of a crossing has to be the frame the settled page
        // draws and the first has to be the frame the listing draws, or the
        // handover is a jump. It was one: the window's own pane went with the
        // page being left, dimmed and shrank through the whole crossing, and
        // came back to full on the frame it ended.
        let mut anim = Anim::default();
        anim.advance(0.0, 0.0, false, 0.0);
        assert_eq!(anim.grown(), 0.0, "a page was already out of its card");
        assert_eq!(anim.arriving(), 0.0, "a page was already solid on its card");

        anim.opened();
        wound(&mut anim, GROW * 2.0);
        assert_eq!(anim.grown(), 1.0, "a page never finished growing");
        assert_eq!(
            anim.arriving(),
            1.0,
            "a page filled the page and was still arriving"
        );

        // And it is solid well before it has finished growing, so most of the
        // crossing is one page growing rather than two at half strength each.
        let mut half = Anim::default();
        half.advance(0.0, 0.0, false, 0.0);
        half.opened();
        wound(&mut half, GROW * 0.5);
        assert_eq!(
            half.arriving(),
            1.0,
            "halfway along, the page was still only {} solid",
            half.arriving()
        );
        assert!(
            half.grown() < 0.9,
            "halfway along, the page had already all but stopped growing: {}",
            half.grown()
        );
    }

    #[test]
    fn going_back_is_the_way_in_run_backwards() {
        // The same clock either way, and from wherever the last one left off:
        // a page turned back halfway leaves from halfway rather than jumping
        // to the far end to leave from there.
        let mut going = Anim::default();
        going.advance(0.0, 0.0, false, 0.0);
        going.opened();
        wound(&mut going, GROW * 0.4);
        let turned = going.grown();

        going.closed();
        wound(&mut going, GROW * 0.4);
        assert!(
            going.grown() < 0.001,
            "a page turned back four tenths of the way in took longer to \
             leave than it had taken to arrive: {} from {turned}",
            going.grown()
        );

        // And every step of the way out is a step of the way in, taken in
        // reverse: at the same distance along, the two add up to the whole of
        // the way. The easing is what makes that true — a curve that was not
        // symmetric about its own middle would leave a page going back at a
        // different speed from the one it came in at.
        let mut out = Anim::default();
        out.advance(0.0, 0.0, false, 0.0);
        out.opened();
        wound(&mut out, GROW * 2.0);
        out.closed();

        let mut into = Anim::default();
        into.advance(0.0, 0.0, false, 0.0);
        into.opened();

        for step in 1..=10 {
            wound(&mut out, FRAME);
            wound(&mut into, FRAME);
            let (leaving, arriving) = (out.grown(), into.grown());
            assert!(
                (leaving + arriving - 1.0).abs() < 0.001,
                "frame {step} of the way out was not frame {step} of the way \
                 in reversed: {leaving} beside {arriving}"
            );
        }
    }

    #[test]
    fn a_page_grows_from_its_card_to_the_whole_of_the_room() {
        let card = [400.0, 300.0, 380.0, 120.0];
        let room = [40.0, 60.0, 1200.0, 800.0];
        assert_eq!(
            between(card, room, 0.0),
            card,
            "it did not start on the card"
        );
        assert_eq!(
            between(card, room, 1.0),
            room,
            "it did not end up filling the page"
        );
        let half = between(card, room, 0.5);
        for (index, name) in ["left", "top", "width", "height"].iter().enumerate() {
            let (from, to) = (card[index], room[index]);
            assert!(
                (half[index] - from).abs() > 0.5 && (half[index] - to).abs() > 0.5,
                "the {name} was at one end of the way rather than halfway \
                 along it: {half:?}"
            );
        }
    }

    #[test]
    fn a_row_nobody_can_see_yet_is_not_drawn() {
        // A listing that has only just been asked for is not worth drawing;
        // one that has settled is.
        let mut anim = Anim::default();
        anim.listed_anew();
        anim.advance(0.0, 0.0, false, 0.0);
        assert!(
            anim.row_in(0, 1.0).fade <= WORTH_DRAWING,
            "a listing was drawn before it had begun to arrive"
        );
        anim.settle(0.0);
        assert!(
            anim.row_in(0, 1.0).fade > WORTH_DRAWING,
            "a listing that had arrived was thrown away as invisible"
        );
        assert_eq!(
            Coming {
                fade: 0.5,
                lean: 7.0
            }
            .moved([10.0, 20.0, 30.0, 40.0]),
            [17.0, 20.0, 30.0, 40.0],
            "a row leaned the wrong way, or in the wrong direction"
        );
    }

    #[test]
    fn a_panel_of_shelves_is_carried_and_then_stops() {
        let mut carried = Carried::default();
        carried.advance(240.0, 1.0 / 60.0);
        assert!(
            carried.at() > 0.0 && carried.at() < 240.0,
            "a panel jumped rather than travelled: {}",
            carried.at()
        );
        for _ in 0..60 {
            carried.advance(240.0, 1.0 / 60.0);
        }
        assert_eq!(carried.at(), 240.0, "a panel was still being carried");
    }

    #[test]
    fn a_light_is_carried_across_a_page_and_put_down_for_a_photograph() {
        let mut light = Light::default();
        // The first rectangle it is ever asked for is where it starts.
        assert_eq!(
            light.glide([0.0, 0.0, 10.0, 10.0], 1.0 / 60.0),
            [0.0, 0.0, 10.0, 10.0],
            "a light appeared somewhere other than where it was put"
        );

        let on_its_way = light.glide([100.0, 0.0, 10.0, 10.0], 1.0 / 60.0);
        assert!(
            on_its_way[0] > 0.0 && on_its_way[0] < 100.0,
            "a light jumped rather than crossing: {on_its_way:?}"
        );

        light.settle();
        assert_eq!(
            light.glide([100.0, 0.0, 10.0, 10.0], 1.0 / 60.0),
            [100.0, 0.0, 10.0, 10.0],
            "a photograph was taken of a light halfway across the page"
        );
    }

    #[test]
    fn a_bar_catches_up_with_a_transaction_rather_than_jumping_to_it() {
        let mut anim = Anim::default();
        anim.advance(0.0, 0.0, false, 0.0);
        anim.advance(1.0 / 60.0, 0.0, false, 1.0);
        assert!(
            anim.through > 0.0 && anim.through < 1.0,
            "a bar arrived in one frame: {}",
            anim.through
        );

        let mut at = 1.0 / 60.0;
        for _ in 0..90 {
            at += 1.0 / 60.0;
            anim.advance(at, 0.0, false, 1.0);
        }
        assert_eq!(anim.through, 1.0, "a bar never finished: {}", anim.through);
    }

    #[test]
    fn a_frame_that_was_a_long_time_coming_does_not_move_everything_a_long_way() {
        let mut anim = Anim::default();
        anim.advance(0.0, 0.0, false, 0.0);
        anim.advance(4.0, 0.0, false, 0.0);
        assert!(
            anim.dt <= 0.1,
            "a window that was not drawn for four seconds moved four seconds' \
             worth in one frame: {}",
            anim.dt
        );
    }

    #[test]
    fn an_indeterminate_bar_goes_across_and_comes_back() {
        let mut anim = Anim::default();
        let mut seen: Vec<f32> = Vec::new();
        let mut at = 0.0;
        for _ in 0..400 {
            at += 1.0 / 30.0;
            anim.advance(at, 0.0, true, 0.0);
            seen.push(anim.sweep());
        }
        let most = seen.iter().cloned().fold(f32::MIN, f32::max);
        let least = seen.iter().cloned().fold(f32::MAX, f32::min);
        assert!(most > 0.95, "a bar never reached the far end: {most}");
        assert!(least < 0.05, "a bar never came back: {least}");
        assert!(
            seen.windows(2).any(|pair| pair[1] < pair[0]),
            "a bar only ever went one way"
        );
    }
}
