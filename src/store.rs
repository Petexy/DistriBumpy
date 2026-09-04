//! The store itself: what is on the screen, what the controls do to it, and
//! what it asks the worker for.
//!
//! The page is `driven`, which means the toolkit hands over the actions and
//! this moves its own light. A store is two lists side by side and a page
//! behind them, and the flow — which numbers controls in the order they are
//! drawn — cannot express that: the light has to be able to sit on a shelf on
//! the left while a listing scrolls on the right.

use std::collections::HashMap;
use std::path::PathBuf;

use lxb_app::lxb_toolkit::input::Action;
use lxb_app::lxb_toolkit::sound::Sound;
use lxb_app::{Page, Typed};

use crate::catalogue::{Catalogue, Link, Section};
use crate::flatpak::{self, Job, Machine, RepoJob, Report, Scope, Worker};
use crate::motion::Anim;
use crate::sandbox::{self, Sandbox, Toggle, TOGGLES};

/// A shelf in the left column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shelf {
    Home,
    Search,
    Updates,
    Installed,
    Section(Section),
    Repositories,
}

impl Shelf {
    pub fn title(self) -> &'static str {
        match self {
            Shelf::Home => "Home",
            Shelf::Search => "Search",
            Shelf::Updates => "Updates",
            Shelf::Installed => "Installed",
            Shelf::Repositories => "Repositories",
            Shelf::Section(section) => section.title(),
        }
    }

    /// What the sidebar calls it, which is not always what the page above the
    /// listing does: the panel is a fixed width and a name that runs out of it
    /// is clipped, which reads as a fault rather than as a long name.
    pub fn short_title(self) -> &'static str {
        match self {
            Shelf::Section(Section::Education) => "Education",
            other => other.title(),
        }
    }

    /// Whether what this shelf lists is a run of applications somebody might
    /// want in another order.
    ///
    /// Home is four short lists of somebody else's choosing and reordering it
    /// would throw away the only thing it says; Repositories is a handful of
    /// rows nobody scrolls. See `Order`.
    pub fn takes_order(self) -> bool {
        matches!(
            self,
            Shelf::Search | Shelf::Updates | Shelf::Installed | Shelf::Section(_)
        )
    }

    /// The order this shelf is in before anybody has asked for one.
    ///
    /// A category is a wall of hundreds of applications most people have never
    /// heard of, and the alphabet says nothing at all about which of them is
    /// worth a press, so **a category opens best rated first**. The other three
    /// start where they always did: Search has a ranking of its own, and
    /// Installed and Updates are short lists of things already chosen, where
    /// the alphabet is how somebody finds the one they came for.
    ///
    /// This is only where a shelf starts. What was asked for outranks it and
    /// is kept across shelves — see `Store::order` — and a shelf that cannot
    /// answer this one yet falls back on its own, which is what a category is
    /// doing while nothing has been heard back from ODRS. See `Order::in_force`.
    pub fn starting_order(self) -> Order {
        match self {
            Shelf::Section(_) => Order::Rating,
            Shelf::Home | Shelf::Search | Shelf::Updates | Shelf::Installed => Order::Best,
            Shelf::Repositories => Order::Best,
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Shelf::Home => "file-home",
            Shelf::Search => "search",
            Shelf::Updates => "refresh",
            Shelf::Installed => "chosen",
            Shelf::Repositories => "file-drive",
            Shelf::Section(section) => section.glyph(),
        }
    }

    /// Whether a rule is drawn above this shelf, which is how the two shelves
    /// that are not a place to browse are set apart from the ten that are.
    pub fn parted_from_the_one_above(self) -> bool {
        matches!(
            self,
            Shelf::Section(Section::Everything) | Shelf::Repositories
        )
    }
}

fn shelves() -> Vec<Shelf> {
    let mut all = vec![Shelf::Home, Shelf::Search, Shelf::Updates, Shelf::Installed];
    all.extend(Section::ALL.map(Shelf::Section));
    all.push(Shelf::Repositories);
    all
}

/// What one row of the listing stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// An application, listed or installed or both.
    App,
    /// The one application promoted at the head of Home.
    ///
    /// It opens exactly as an application does, but takes a line of its own so
    /// drawing and controller navigation agree about the hero's shape.
    Featured,
    /// A repository, on the shelf that lists them.
    Repo { scope: Scope, disabled: bool },
    /// The row at the head of the repositories shelf.
    Add,
    /// A row at the head of the updates shelf, for one installation.
    UpdateAll { scope: Scope },
    /// The row at the head of the installed shelf.
    Trim,
    /// A runtime, an SDK or an extension: one of the things applications
    /// stand on, which Discover calls Application Support.
    ///
    /// It is on the updates shelf and nowhere else, because that is the one
    /// page where what is about to be fetched has to be named. It does not
    /// open a page — there is no page to open, since nothing in a catalogue
    /// describes a runtime — and a press on one updates that one thing.
    Support { scope: Scope },
    /// A line of type across the listing, naming what is under it. Nothing
    /// rests on one and pressing cannot reach one.
    Heading,
}

impl Kind {
    /// Whether this row opens an application's page.
    pub fn is_app(&self) -> bool {
        matches!(self, Kind::App | Kind::Featured)
    }

    /// Whether this is one of the rows that acts rather than opens.
    ///
    /// A head row sits above a list and does something to the whole of it. It
    /// is a row rather than a button because a column of rows the light walks
    /// is already how everything else on the page is reached, and because a
    /// button above an empty column is a button nothing explains.
    pub fn is_head(&self) -> bool {
        matches!(self, Kind::Add | Kind::UpdateAll { .. } | Kind::Trim)
    }

    /// Whether this takes a line of its own rather than a place in the grid.
    pub fn is_wide(&self) -> bool {
        self.is_head() || matches!(self, Kind::Featured | Kind::Heading | Kind::Support { .. })
    }

    /// Whether the light can rest on it.
    pub fn can_be_chosen(&self) -> bool {
        *self != Kind::Heading
    }

    /// Whether a pointer press should open this row straight away.
    ///
    /// Rows that start work keep the existing focus-then-press behaviour: a
    /// click must not quietly turn an Update all or cleanup row into a one-click
    /// operation. Cards that lead to another page behave like links and open on
    /// the click that names them.
    pub fn opens_on_click(&self) -> bool {
        matches!(
            self,
            Kind::App | Kind::Featured | Kind::Repo { .. } | Kind::Add
        )
    }
}

/// What one line of the listing is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// The promoted application at the head of Home.
    Hero,
    /// A line of type naming what is under it.
    Heading,
    /// One row across the whole width, which is what a head row is.
    Wide,
    /// Up to as many cards as there is room for.
    Cells,
}

/// The shape a frame laid the listing out in, as drawing measured it.
///
/// One value rather than six arguments, because they are one fact: six
/// measurements that only mean anything apart from one another, handed over at
/// one place.
#[derive(Debug, Clone)]
pub struct Shape {
    /// How many cards came out across the grid.
    pub columns: usize,
    /// How many whole lines fitted below the one at the top.
    pub room: usize,
    /// Where each line begins, in points down the listing.
    pub tops: Vec<f32>,
    /// The air the listing is held back from its own top edge by.
    pub peek: f32,
    /// How tall the listing is on the page.
    pub viewport: f32,
    /// How deep the whole listing runs.
    pub deep: f32,
}

/// One line of the listing, and which rows are on it.
#[derive(Debug, Clone)]
pub struct Line {
    pub kind: LineKind,
    pub rows: Vec<usize>,
}

/// How a listing of applications is ordered.
///
/// Discover's five, as near as this machine can honestly answer them, and one
/// of its own. There is no "most downloaded": Flathub publishes no such number
/// to this machine, and a store must not invent one — the same reason Home
/// answers "best rated" with `trending`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Order {
    /// Whatever the shelf itself thinks best: how well a search matched, and
    /// by name everywhere else, which is the order the catalogue is read in.
    #[default]
    Best,
    /// What people gave it, weighted so that a shelf is not ordered by who has
    /// the fewest opinions about them. See `ratings::Rating::score`.
    Rating,
    /// How many people said anything at all.
    Reviews,
    /// What it takes up on this disk. Offered only where every row has one:
    /// the catalogue declares no sizes, so what an application *would* cost is
    /// not known until a transaction has been out and resolved it, and a shelf
    /// ordered by a number nobody has is a shelf ordered by nothing.
    Size,
    /// The newest declared release first.
    Newest,
    /// The applications whose publisher the remote vouches for, first.
    Verified,
}

/// Every order, in the order a menu lists them. `Best` is first because it is
/// where a shelf starts.
pub const ORDERS: [Order; 6] = [
    Order::Best,
    Order::Rating,
    Order::Reviews,
    Order::Size,
    Order::Newest,
    Order::Verified,
];

impl Order {
    /// What a menu calls it, which depends on the shelf for exactly one of
    /// them: `Best` is a ranking on Search and is plain alphabetical order
    /// everywhere else, and naming it "Best match" over a category would be
    /// naming something the shelf does not do.
    pub fn title(self, shelf: Shelf) -> &'static str {
        match self {
            Order::Best if shelf == Shelf::Search => "Best match",
            Order::Best => "Name (A to Z)",
            Order::Rating => "Rating (highest first)",
            Order::Reviews => "Reviews (most first)",
            Order::Size => "Size (largest first)",
            Order::Newest => "Released (newest first)",
            Order::Verified => "Verified publishers first",
        }
    }

    /// The same thing said in the corner beside how many there are, where it
    /// has to be short enough to sit at the end of a line.
    pub fn shown(self, shelf: Shelf) -> &'static str {
        match self {
            Order::Best if shelf == Shelf::Search => "best match",
            Order::Best => "by name",
            Order::Rating => "best rated",
            Order::Reviews => "most reviewed",
            Order::Size => "largest first",
            Order::Newest => "newest first",
            Order::Verified => "verified first",
        }
    }

    /// Whether this shelf can really answer this one.
    ///
    /// The rule the whole page keeps: nothing is offered that does nothing
    /// where it is offered. Size is the one that is not universal — see the
    /// note on the variant — and the two that read what people said are only
    /// worth offering once anything has been heard back at all.
    pub fn can_answer(self, shelf: Shelf, rated: bool) -> bool {
        match self {
            Order::Best | Order::Newest | Order::Verified => true,
            Order::Rating | Order::Reviews => rated,
            Order::Size => matches!(shelf, Shelf::Installed | Shelf::Updates),
        }
    }

    /// The order really in force, out of what was asked for and what this
    /// shelf can really answer.
    ///
    /// `asked` is `None` until somebody asks, and then the shelf's own
    /// starting order stands. Either way, an order this shelf cannot answer
    /// falls back to `Best` rather than pretending to: Size is asked for on
    /// Installed and means nothing on a category, and a category starts best
    /// rated but is by name until ODRS has answered at all.
    pub fn in_force(asked: Option<Order>, shelf: Shelf, rated: bool) -> Order {
        let order = asked.unwrap_or_else(|| shelf.starting_order());
        if order.can_answer(shelf, rated) {
            order
        } else {
            Order::Best
        }
    }
}

/// One row of the listing, already carrying everything drawing it needs.
///
/// Built when the shelf or the query changes rather than every frame: the
/// widest shelf is every application on Flathub, and filtering three thousand
/// of them sixty times a second to draw twelve would be work nobody sees.
#[derive(Debug, Clone)]
pub struct Row {
    pub id: String,
    pub name: String,
    pub summary: String,
    /// Who publishes the application, where AppStream names them.
    pub developer: String,
    pub icon: Option<PathBuf>,
    /// Where the icon can be fetched from, for the listings whose remote
    /// promised one and never wrote it to this disk.
    pub icon_url: Option<String>,
    /// The first catalogue screenshot, used by Home's promoted application.
    pub screenshot: Option<crate::catalogue::Shot>,
    pub kind: Kind,
    pub installed: bool,
    pub updatable: bool,
    /// Whether the remote vouches for the publisher. Carried on the row rather
    /// than looked up, because `Order::Verified` reads it for every row of a
    /// four-hundred-long shelf on every rebuild.
    pub verified: bool,
    /// When its newest release was published, as seconds. Nought where the
    /// catalogue never said — see `Order::Newest`.
    pub released: i64,
    /// What people gave it, and how many of them, where anybody has. Nought
    /// where nobody has — see `Order::Rating` and `Order::Reviews`.
    pub rating: f32,
    pub reviews: u32,
    /// What it takes up on this disk, for the shelves whose rows are all
    /// installed. Nought everywhere else: the catalogue declares no sizes, so
    /// what an application would cost is not known until a transaction has
    /// been out and resolved it — see `Order::Size`.
    pub size: u64,
    /// The mark to draw where there is no icon, which is how a head row and a
    /// repository are drawn at all.
    pub glyph: &'static str,
}

impl Row {
    /// A line of type across the listing.
    pub fn heading(name: &str, note: &str) -> Self {
        Self {
            id: String::new(),
            name: name.to_string(),
            summary: note.to_string(),
            developer: String::new(),
            icon: None,
            icon_url: None,
            screenshot: None,
            kind: Kind::Heading,
            installed: false,
            updatable: false,
            verified: false,
            released: 0,
            rating: 0.0,
            reviews: 0,
            size: 0,
            glyph: "launch",
        }
    }
}

/// Which column the light is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Shelves,
    /// The search field, which is a place the light stands in rather than a
    /// row of the listing.
    ///
    /// It has to be one: an on-screen keyboard is summoned by a client saying
    /// that a field *inside* its window has the cursor, and nothing can say
    /// that of a page which merely happens to be showing a field. Only the
    /// Search shelf has one.
    Field,
    Listing,
}

/// What is on the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Browse,
    Detail { id: String },
    Repository { name: String, scope: Scope },
    AddRepository,
}

/// How far along a track a pointer has taken a thumb, from nought to one.
///
/// **The thumb's middle follows the pointer**, which is why the room it has to
/// move in is the track less the thumb rather than the track. Measured from
/// its top corner instead, a bar taken hold of anywhere but its very top jumps
/// by up to its own length before it begins to move.
pub fn share_along(track: [f32; 4], thumb: f32, y: f32) -> f32 {
    let room = track[3] - thumb;
    if room <= 0.0 {
        return 0.0;
    }
    ((y - track[1] - thumb * 0.5) / room).clamp(0.0, 1.0)
}

/// Which band of a page the light is in.
///
/// A detail page is three bands stacked: what can be done, what can be read,
/// and the reading itself. Up and Down cross between them and Left and Right
/// move within one, which is the shell's own idiom and the only one that works
/// on a controller with four directions and no pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    Buttons,
    Tabs,
    Content,
}

/// Where a direction takes the light out of the top two bands of a detail
/// page, where it takes it out of them at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Crossing {
    /// Into that band, wherever the light last stood in it.
    To(Band),
    /// Back along the row to the last control — the step Left makes off the
    /// front of the tabs, which is not the same as arriving from above.
    ToLastControl,
}

/// Where a direction out of the controls or the tabs lands.
///
/// `None` means the band keeps the light and walks within itself.
///
/// **A direction has to mean what the page looks like it means.** The controls
/// and the tabs are two bands of the page's model and are drawn on one line
/// wherever they fit, so Down through the controls would walk the light
/// sideways — the fault every merged row invites. On one line they are walked
/// as one: Right off the end of the controls is the tabs, Left off the front
/// of the tabs is the last control, Down from either is the reading below, and
/// Up out of the tabs is nothing, because there is nothing above them. Wrapped
/// onto two lines, which is what a narrow window and four controls come to,
/// they go back to being two bands stacked.
///
/// Pure, because this is the whole of what the merged row changed and the only
/// part of it worth a test on its own: acting needs a `Page` to make a sound
/// with, and this needs nothing.
pub fn crossing(
    band: Band,
    action: Action,
    one_row: bool,
    button: usize,
    buttons: usize,
    tab: usize,
) -> Option<Crossing> {
    match (band, action) {
        (Band::Buttons, Action::Down) => Some(Crossing::To(if one_row {
            Band::Content
        } else {
            Band::Tabs
        })),
        (Band::Buttons, Action::Right) if one_row && button + 1 >= buttons => {
            Some(Crossing::To(Band::Tabs))
        }
        (Band::Tabs, Action::Up) if one_row => None,
        (Band::Tabs, Action::Up) => Some(Crossing::To(Band::Buttons)),
        (Band::Tabs, Action::Down | Action::Accept) => Some(Crossing::To(Band::Content)),
        (Band::Tabs, Action::Left) if one_row && tab == 0 && buttons > 0 => {
            Some(Crossing::ToLastControl)
        }
        _ => None,
    }
}

/// Where Up out of the reading lands.
///
/// Wrapped onto two lines the answer is always the tabs: they are the row
/// directly above, and the light walks back up one row at a time exactly as it
/// walked down.
///
/// Merged onto one line it has to be whichever band the light really came down
/// from. The controls and the tabs share that line but are two bands, so
/// coming back to the wrong one moves the light sideways to a different
/// control — Down from Remove and Up again landed on About, which is not where
/// anybody left it.
///
/// Pure, for the same reason [`crossing`] is: this is the whole of the rule,
/// and acting on it needs a `Page` only to make a sound with.
pub fn back_up(one_row: bool, came_down_from: Band, buttons: usize) -> Band {
    if one_row && came_down_from == Band::Buttons && buttons > 0 {
        Band::Buttons
    } else {
        Band::Tabs
    }
}

/// Which part of an application a detail page is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    About,
    Changes,
    Permissions,
    Links,
}

impl Tab {
    pub fn title(self) -> &'static str {
        match self {
            Tab::About => "About",
            Tab::Changes => "What's new",
            Tab::Permissions => "Permissions",
            Tab::Links => "Links",
        }
    }
}

/// A job in flight, and the last thing the worker said about it.
pub struct Running {
    pub job: Job,
    pub name: String,
    pub step: String,
    pub through: f32,
    pub at: usize,
    pub of: usize,
    pub transferred: u64,
    /// Set once a stop has been asked for, so the bar can say so rather than
    /// looking like it has hung.
    pub stopping: bool,
}

/// What the store is waiting to be told, if anything.
pub enum Reading {
    /// The first read, which is the one the user waits through.
    First(std::sync::mpsc::Receiver<(Machine, Catalogue)>),
    /// A re-read after something was installed or removed. The old catalogue
    /// stays on screen while it runs, because it is still true.
    Again(std::sync::mpsc::Receiver<(Machine, Catalogue)>),
}

/// A destructive action waiting for an explicit answer from the dialog.
/// Keeping the exact target here means a page changing underneath the dialog
/// cannot make the answer apply to something else.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Confirmation {
    Remove { id: String },
    Forget { name: String, scope: Scope },
}

/// What a new repository is being given.
#[derive(Debug, Default)]
pub struct Adding {
    pub url: String,
    pub name: String,
    /// Set once somebody has typed a name of their own, after which it stops
    /// being taken from the address.
    named: bool,
    /// Whether the address is being written into **now**.
    ///
    /// The row is a control that is pressed, not a field the light happens to
    /// be standing in, and this is what a press turns on. Two reasons. The
    /// column it is in is four rows that are pressed to add a repository and
    /// this one; a fifth that silently swallowed letters instead was the odd
    /// one out. And an on-screen keyboard is raised by the window saying a
    /// field inside it has the cursor — so that has to become true at a
    /// moment, on a press, rather than be true for the whole screen from the
    /// instant it opens, which is what it was.
    pub typing: bool,
}

impl Adding {
    /// The name a repository would be filed under, taken from the last part of
    /// its address the way `flatpak remote-add` does.
    fn name_from(url: &str) -> String {
        url.rsplit('/')
            .next()
            .unwrap_or_default()
            .trim_end_matches(".flatpakrepo")
            .trim_end_matches(".flatpakref")
            .to_string()
    }

    fn typed_url(&mut self, url: String) {
        self.url = url;
        if !self.named {
            self.name = Self::name_from(&self.url);
        }
    }

    /// Whether there is enough here to try.
    pub fn ready(&self) -> bool {
        self.url.starts_with("https://") && !self.name.trim().is_empty()
    }
}

pub struct Store {
    pub machine: Machine,
    pub catalogue: Catalogue,
    pub reading: Option<Reading>,

    pub screen: Screen,
    /// The page being left, while it is still shrinking back into its card.
    ///
    /// Nothing vanishes before its transition ends, and a page going back is
    /// still on the screen for as long as it takes to get there. `screen` is
    /// already `Browse` — the listing is what the presses go to — and this is
    /// only what is still drawn over it. See [`Store::over`].
    leaving: Option<Screen>,
    /// Where every card in the listing came out this frame, by the row it is,
    /// so that a press can be told which rectangle the page it opens grows
    /// out of. Written down by drawing; see [`Store::cards_are`].
    cards: Vec<(usize, [f32; 4])>,
    /// The row a page was opened out of — which row it was and what it names
    /// — and where its card is now.
    ///
    /// The card is looked up again every frame the listing is drawn rather
    /// than remembered from the press, so a window resized while a page is
    /// open still shrinks back into the rectangle its card has ended up in.
    /// What it names is only the fallback for a rebuild behind an open page
    /// having moved it: the head rows of a shelf all carry an empty
    /// identifier, and they are not one another.
    opened_id: Option<String>,
    opened_row: Option<usize>,
    opened_from: Option<[f32; 4]>,
    pub column: Column,
    pub shelf: usize,
    /// The first shelf drawn in the panel, how many fitted in it, and how
    /// tall one of them is.
    ///
    /// There are fifteen shelves and a panel is as tall as the window is. They
    /// were shared out over whatever room there was until the rows were half
    /// the height of the language's own, which is what made the panel read as
    /// cramped; now they are drawn at their proper size and the panel scrolls.
    pub shelf_top: usize,
    pub shelf_room: usize,
    pub shelf_height: f32,
    pub row: usize,
    /// The first **line** drawn, which is what makes a list of three thousand
    /// fit on a screen that holds a dozen.
    pub top: usize,
    /// How many lines the last frame had room for, and how far down the
    /// listing each line begins. Drawing works both out, and moving and
    /// scrolling need them, so they are written down as the page is drawn.
    pub room: usize,
    pub line_tops: Vec<f32>,
    /// The listing's own half of [`Store::shelf_peek`].
    pub peek: f32,
    /// How tall the listing is on the page, and how deep the whole of it runs.
    /// Where the light can rest without the list moving under it is worked out
    /// from these and `line_tops`, in points.
    pub viewport: f32,
    pub deep: f32,
    /// How many cards fit across. Drawing works this out from the width, and
    /// the lines are built from it.
    pub columns: usize,

    pub query: String,
    pub rows: Vec<Row>,
    /// The rows laid out into lines, which is what the light moves over.
    pub lines: Vec<Line>,
    /// Set when the shelf, the query or the machine changed and `rows` no
    /// longer says the truth.
    stale: bool,

    /// Which band of a detail page the light is in, and where in it.
    pub band: Band,
    pub button: usize,
    pub tab: usize,
    /// Where the content band is scrolled to, or which of its rows is chosen.
    pub content: usize,
    /// The first content row drawn, which is what makes a band of twenty rows
    /// fit in room for eight.
    pub content_top: usize,
    /// How many rows the content band had room for last frame, and how many
    /// steps it has in all. Drawing works both out, and moving needs them, so
    /// they are written down as the page is drawn.
    pub content_room: usize,
    content_deep: usize,
    /// Whether the controls and the tabs came out on one line, which drawing
    /// works out and moving has to know. See [`Store::one_row_is`].
    one_row: bool,
    /// Which of the two bands above the reading the light came down from, so
    /// that Up can put it back where it was. See [`back_up`].
    came_down_from: Band,
    /// Where the two bars a pointer can take hold of came out, and how long
    /// the thumb on each was: the track, then the thumb.
    ///
    /// The same bargain as `shape_is`. Only drawing knows where it put them,
    /// and dragging one needs both — where the track runs, and how much of it
    /// the thumb itself takes up, which is what is left for the thumb to move
    /// in. A bar that was not drawn leaves a track of no height, which nothing
    /// can be dragged along.
    listing_track: ([f32; 4], f32),
    content_track: ([f32; 4], f32),
    /// Which screenshot is shown.
    pub shot: usize,

    /// What a not-yet-installed application would cost, once the worker has
    /// been out and resolved it.
    pub weights: HashMap<String, (u64, u64)>,
    weighing: Option<String>,

    /// The sandbox of whichever application is being looked at.
    sandbox: Option<(String, Sandbox)>,

    pub adding: Adding,

    /// The order that was **asked for**, kept across shelves: somebody who
    /// asked for the newest first meant it about the way they read a shelf,
    /// not about the one shelf they happened to be on.
    ///
    /// `None` until somebody asks, which is what leaves room for a shelf to
    /// open in an order of its own — see `Shelf::starting_order`.
    pub order: Option<Order>,

    pub running: Option<Running>,
    confirming: Option<Confirmation>,
    /// Set only for the action that opened a confirmation. The frame loop uses
    /// it to discard any further actions that were queued before the modal
    /// existed.
    opened_modal: bool,
    /// The last thing to go wrong, until something else happens.
    pub trouble: Option<String>,
    /// The last thing to go right that was worth a sentence.
    pub note: Option<String>,

    pub worker: Worker,
    pub art: crate::art::Art,
    pub anim: Anim,
    /// What Flathub says is worth looking at, which is the one thing on any of
    /// these pages that cannot be read off this disk.
    pub flathub: crate::flathub::Collections,
    /// What people who have used these applications think of them. Not from
    /// the remote the applications come from — see `src/ratings.rs`.
    pub ratings: crate::ratings::Ratings,
}

impl Store {
    pub fn new() -> Self {
        Self::with(Some(Reading::First(read_in_the_background())))
    }

    /// A store that has not gone looking for the machine.
    ///
    /// `Store::new` starts a thread inside flatpak's own libraries, and a test
    /// that is over in a millisecond leaves that thread running into process
    /// exit — which segfaults often enough to be seen. Nothing that only asks
    /// what a page's controls are needs the machine read at all.
    #[cfg(test)]
    fn quiet() -> Self {
        Self::with(None)
    }

    fn with(reading: Option<Reading>) -> Self {
        Self {
            machine: Machine::default(),
            catalogue: Catalogue::default(),
            reading,
            screen: Screen::Browse,
            leaving: None,
            cards: Vec::new(),
            opened_id: None,
            opened_row: None,
            opened_from: None,
            column: Column::Shelves,
            shelf: 0,
            shelf_top: 0,
            shelf_room: 1,
            shelf_height: 0.0,
            row: 0,
            top: 0,
            room: 1,
            line_tops: Vec::new(),
            peek: 0.0,
            viewport: 0.0,
            deep: 0.0,
            columns: 1,
            query: String::new(),
            rows: Vec::new(),
            lines: Vec::new(),
            stale: true,
            band: Band::Buttons,
            button: 0,
            tab: 0,
            content: 0,
            content_top: 0,
            content_room: 1,
            content_deep: 1,
            one_row: true,
            came_down_from: Band::Tabs,
            listing_track: ([0.0; 4], 0.0),
            content_track: ([0.0; 4], 0.0),
            shot: 0,
            weights: HashMap::new(),
            weighing: None,
            sandbox: None,
            adding: Adding::default(),
            running: None,
            order: None,
            confirming: None,
            opened_modal: false,
            trouble: None,
            note: None,
            worker: Worker::start(),
            art: crate::art::Art::new(),
            anim: Anim::default(),
            flathub: crate::flathub::Collections::new(),
            ratings: crate::ratings::Ratings::new(),
        }
    }

    /// A store that has already read the machine, for a picture of a page.
    ///
    /// `App::shot` draws two frames and stops, so a store that read its
    /// catalogue on a thread would be photographed saying it was still
    /// reading. Nothing else waits like this.
    pub fn ready() -> Self {
        let mut store = Self::new();
        let waited = std::time::Instant::now();
        while store.opening() && waited.elapsed() < std::time::Duration::from_secs(60) {
            store.advance();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        // And for what people said, where that is still on its way: a picture
        // of a shelf sorted by rating taken before the ratings arrived is a
        // picture of a shelf sorted by nothing.
        let waited = std::time::Instant::now();
        while store.ratings.waiting() && waited.elapsed() < std::time::Duration::from_secs(45) {
            store.advance();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        store.advance();
        store
    }

    /// Wait for one picture to arrive, for the same reason.
    pub fn wait_for_art(&mut self, url: &str) {
        let waited = std::time::Instant::now();
        while self.art.picture(url).is_none()
            && waited.elapsed() < std::time::Duration::from_secs(30)
        {
            self.art.advance();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    /// Put the store on a page, for a picture of it.
    pub fn look_at(&mut self, shelf: usize, row: Option<usize>, id: Option<&str>, query: &str) {
        self.shelf = shelf.min(shelves().len() - 1);
        // A shelf arrived at fresh: the light at its top, which is where
        // walking to one leaves it. Without this, a store that had already
        // listed Home — whose first row is a line of type, so the light steps
        // past it — opens every other shelf one row down.
        self.row = 0;
        self.top = 0;
        self.query = query.to_string();
        self.stale = true;
        self.rebuild();
        if self.shelf() == Shelf::Search {
            self.column = Column::Field;
        }
        if let Some(row) = row {
            self.column = Column::Listing;
            // Past a line of type, the way walking to it would go: a heading
            // names what is under it and the light never rests on one.
            let want = row.min(self.rows.len().saturating_sub(1));
            self.row = (want..self.rows.len())
                .chain((0..want).rev())
                .find(|at| self.rows[*at].kind.can_be_chosen())
                .unwrap_or(want);
            // Left at the top rather than settled here: how many rows there is
            // room for is not known until a frame has been laid out, and
            // settling against a guess of one scrolls a list that fits.
            self.top = 0;
        }
        if let Some(id) = id {
            self.screen = Screen::Detail { id: id.to_string() };
            // Standing on the page, not on its way to it: `Anim::settle` puts
            // the crossing wherever it is going, and where it is going is
            // whichever way it was last sent.
            self.anim.opened();
            let urls: Vec<String> = self
                .catalogue
                .get(id)
                .map(|listing| {
                    listing
                        .screenshots
                        .iter()
                        .map(|shot| shot.url.clone())
                        .collect()
                })
                .unwrap_or_default();
            for url in urls {
                self.wait_for_art(&url);
            }
            // What an install would really cost takes a round trip to the
            // remote. A picture of the page has to wait for it, or it is a
            // picture of the page still asking.
            self.weigh(id);
            let waited = std::time::Instant::now();
            while self.weighing(id) && waited.elapsed() < std::time::Duration::from_secs(30) {
                self.advance();
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        // Nothing will carry this page in: `App::shot` draws twice at one
        // instant, so everything has to be put where it is going.
        self.anim.settle(self.scroll_target());
    }

    /// Put everything where it is going, once a frame has been drawn.
    ///
    /// How many rows there is room for is only known after a frame has been
    /// laid out, and where a list is scrolled to depends on that. `App::shot`
    /// draws twice at one instant: this is what the first of those two frames
    /// is for.
    pub fn settle_for_a_picture(&mut self) {
        self.settle_the_window();
        self.settle_the_shelves();
        self.art.settle();
        self.anim.settle(self.scroll_target());
        self.anim.shelves.settle(self.shelf_scroll_target());
    }

    /// Put the store on one of a detail page's tabs, for a picture of it.
    pub fn look_at_tab(&mut self, tab: usize) {
        self.tab = tab;
        if tab > 0 {
            self.band = Band::Content;
        }
        self.anim.settle(self.scroll_target());
    }

    /// Put the store on a repository's page, for a picture of it.
    pub fn look_at_repository(&mut self, name: &str) {
        self.shelf = shelves().len() - 1;
        self.stale = true;
        self.rebuild();
        if let Some(remote) = self
            .machine
            .listed_remotes()
            .into_iter()
            .find(|one| one.name == name)
        {
            self.screen = Screen::Repository {
                name: remote.name.clone(),
                scope: remote.scope,
            };
            self.anim.opened();
        }
        self.column = Column::Listing;
        self.anim.settle(self.scroll_target());
    }

    /// Put the store on the page a repository is added from.
    pub fn look_at_adding(&mut self) {
        self.shelf = shelves().len() - 1;
        self.stale = true;
        self.rebuild();
        self.screen = Screen::AddRepository;
        self.anim.opened();
        self.adding.typing = false;
        self.band = Band::Content;
        self.anim.settle(self.scroll_target());
    }

    pub fn shelf(&self) -> Shelf {
        shelves()
            .get(self.shelf)
            .copied()
            .unwrap_or(Shelf::Section(Section::Everything))
    }

    pub fn shelves(&self) -> Vec<Shelf> {
        shelves()
    }

    /// Whether the first read has not finished, which is the one state where
    /// the store has nothing at all to show.
    pub fn opening(&self) -> bool {
        matches!(self.reading, Some(Reading::First(_)))
    }

    /// Whether the machine is being read again behind whatever is on screen.
    pub fn rereading(&self) -> bool {
        matches!(self.reading, Some(Reading::Again(_)))
    }

    /// The page standing over the listing, if there is one.
    ///
    /// Not the same question as which screen the presses go to. On the way
    /// out those two part company for as long as the crossing lasts: the
    /// listing has the presses back from the moment Back is pressed, and the
    /// page being left is still drawn over it until it has shrunk into its
    /// card.
    pub fn over(&self) -> Option<Screen> {
        match &self.screen {
            Screen::Browse => self.leaving.clone(),
            other => Some(other.clone()),
        }
    }

    /// Whether a page is on its way out of a card or back into one.
    pub fn in_the_crossing(&self) -> bool {
        let over = !matches!(self.screen, Screen::Browse) || self.leaving.is_some();
        over && !self.anim.filling_the_page()
    }

    /// Where the card the page over the listing grew out of is now.
    pub fn opened_from(&self) -> Option<[f32; 4]> {
        self.opened_from
    }

    /// Where drawing put the cards this frame.
    ///
    /// The same bargain as `shape_is`: only drawing knows where it put them,
    /// and a press has to know which rectangle to grow a page out of. Where
    /// it is *really* drawn rather than where it belongs, so a page opened by
    /// a card still dipping under the press grows out of the dipped card and
    /// not out of a rectangle a little larger than the one on the screen.
    pub fn cards_are(&mut self, places: Vec<(usize, [f32; 4])>) {
        self.cards = places;
        let Some(id) = self.opened_id.clone() else {
            return;
        };
        // The row it was pressed on, and what that row names only as a
        // fallback for a rebuild behind the open page having moved it. Two
        // rows can name the same application — a hero at the head of a shelf
        // and a card of it further down the same one — and the page belongs
        // to the one that was really pressed. A row naming nothing at all
        // never answers to the fallback: every head row of a shelf carries an
        // empty identifier, and Add a repository is not Update everything.
        let mut found = None;
        for (row, at) in &self.cards {
            let names_it = self.rows.get(*row).is_some_and(|one| one.id == id);
            if Some(*row) == self.opened_row && names_it {
                found = Some(*at);
                break;
            }
            if names_it && !id.is_empty() {
                found = found.or(Some(*at));
            }
        }
        if let Some(at) = found {
            self.opened_from = Some(at);
        }
    }

    /// Grow the page a card opens out of that card.
    fn out_of_the_card(&mut self, id: &str) {
        self.leaving = None;
        self.opened_id = Some(id.to_string());
        self.opened_row = Some(self.row);
        self.opened_from = self
            .cards
            .iter()
            .find(|(row, _)| *row == self.row)
            .map(|(_, at)| *at);
        self.anim.opened();
    }

    /// Shrink whatever is over the listing back into the card it came out of,
    /// and give the listing the presses back.
    fn back_into_the_card(&mut self) {
        if !matches!(self.screen, Screen::Browse) {
            self.leaving = Some(self.screen.clone());
            self.screen = Screen::Browse;
        }
        self.anim.closed();
    }

    /// Take in anything a thread has said since the last frame.
    pub fn advance(&mut self) {
        self.art.advance();
        self.flathub.advance();
        if self.flathub.changed && self.shelf() == Shelf::Home {
            self.stale = true;
        }
        self.ratings.advance();
        if self.ratings.changed {
            // Every row carries its own, so the shelf has to be built again
            // for an answer that arrived after it was.
            self.stale = true;
        }
        self.hear_the_readers();
        self.hear_the_worker();
        if self.stale {
            self.rebuild();
        }
    }

    /// Move everything that is on its way somewhere, once a frame.
    pub fn animate(&mut self, seconds: f32) {
        let through = self
            .running
            .as_ref()
            .map(|running| running.through)
            .unwrap_or(0.0);
        let target = self.scroll_target();
        self.anim.advance(seconds, target, self.opening(), through);
        // Nothing is let go until it lands: the page being left is dropped
        // here, on the frame after the one it finished shrinking on, rather
        // than at the press that started it back.
        if self.anim.on_the_card() {
            self.leaving = None;
        }
        let dt = self.anim.dt;
        let shelves = self.shelf_scroll_target();
        self.anim.shelves.advance(shelves, dt);
    }

    fn hear_the_readers(&mut self) {
        let Some(reading) = &self.reading else {
            return;
        };
        let waiting = match reading {
            Reading::First(waiting) | Reading::Again(waiting) => waiting,
        };
        match waiting.try_recv() {
            Ok((machine, catalogue)) => {
                self.machine = machine;
                self.catalogue = catalogue;
                self.reading = None;
                self.stale = true;
                // What an application may reach can have changed under this,
                // and the page is about to be asked.
                self.sandbox = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.reading = None;
                if self.machine.trouble.is_none() {
                    self.machine.trouble = Some("The catalogue could not be read.".into());
                }
            }
        }
    }

    fn hear_the_worker(&mut self) {
        for report in self.worker.heard() {
            match report {
                // A transaction installing one application runs an operation
                // for every runtime and extension it needs, so a step is about
                // whichever of them is being fetched. Only an ending has to be
                // about the job that was asked for.
                Report::Step { what, at, of } => {
                    if let Some(running) = &mut self.running {
                        running.step = what;
                        running.at = at;
                        running.of = of;
                    }
                }
                Report::Progress {
                    through,
                    transferred,
                    status,
                } => {
                    if let Some(running) = &mut self.running {
                        running.through = through;
                        running.transferred = transferred;
                        if !status.is_empty() {
                            running.step = status;
                        }
                    }
                }
                Report::Weighed {
                    reference,
                    download,
                    installed,
                } => {
                    if let Some(id) = reference.split('/').nth(1) {
                        self.weights.insert(id.to_string(), (download, installed));
                    }
                    self.weighing = None;
                }
                Report::Done { about, said } => {
                    if self.weighing.as_deref() == Some(about.as_str()) {
                        // Arithmetic finishing changes nothing on this
                        // machine, so nothing is re-read for it.
                        self.weighing = None;
                    } else if self.about_this_job(&about) {
                        self.running = None;
                        if !said.is_empty() {
                            self.note = Some(said);
                        }
                        self.reread();
                    }
                }
                Report::Failed { about, why } => {
                    if self.weighing.as_deref() == Some(about.as_str()) {
                        // A size that could not be worked out is not worth
                        // interrupting anybody over: the page simply stops
                        // saying it is working it out.
                        self.weighing = None;
                    } else if self.about_this_job(&about) {
                        self.running = None;
                        if why.starts_with("Stopped.") {
                            self.note = Some(why);
                            self.trouble = None;
                        } else {
                            self.trouble = Some(why);
                        }
                        self.reread();
                    } else if self.running.is_none() {
                        // Something that ran without a bar — starting an
                        // application is the only one — and did not work.
                        self.trouble = Some(why);
                    }
                }
            }
        }
    }

    /// Whether an ending is about the job the bar on screen belongs to.
    ///
    /// A report arriving after the next job was started, or one belonging to
    /// something that never had a bar, must not clear a bar belonging to
    /// something else — and must not send this store off to read the whole
    /// machine again for nothing.
    fn about_this_job(&self, about: &str) -> bool {
        self.running
            .as_ref()
            .is_some_and(|running| running.job.about() == about)
    }

    /// Read the machine again, keeping what is on screen until it answers.
    fn reread(&mut self) {
        self.sandbox = None;
        if self.reading.is_none() {
            self.reading = Some(Reading::Again(read_in_the_background()));
        }
    }

    fn rebuild(&mut self) {
        self.stale = false;
        let before = self.rows.len();
        self.rows = match self.shelf() {
            Shelf::Home => self.home_rows(),
            Shelf::Search => self
                .catalogue
                .search(&self.query)
                .into_iter()
                .take(400)
                .map(|listing| self.row_of_listing(listing))
                .collect(),
            Shelf::Installed => {
                let left_over = self.machine.unused_count;
                let mut rows = vec![head_row(
                    Kind::Trim,
                    "Clear out what nothing needs",
                    &if left_over == 0 {
                        "Nothing installed for this user is left over".to_string()
                    } else {
                        format!(
                            "{left_over} left over for this user, {} of disk",
                            flatpak::size(self.machine.unused)
                        )
                    },
                    "uninstall",
                )];
                // Applications, which is what a list of installed
                // applications is. The runtimes and extensions under them are
                // on the updates shelf, where what is about to be fetched has
                // to be named, and nowhere else: two hundred of them here
                // would bury the twenty things somebody actually chose.
                let mut installed: Vec<flatpak::Installed> = self.machine.apps().cloned().collect();
                installed.sort_by_key(|one| one.name.to_lowercase());
                rows.extend(installed.iter().map(|one| self.row_of_installed(one)));
                rows
            }
            Shelf::Updates => {
                let stale = self.machine.updatable();
                let mut rows = Vec::new();
                for scope in [Scope::User, Scope::System] {
                    let count = stale.iter().filter(|one| one.scope == scope).count();
                    if count == 0 {
                        continue;
                    }
                    // Not "apps": what this fetches is everything in one
                    // installation that has a newer commit waiting, runtimes
                    // and extensions included — which is what the shelf under
                    // it now says as well.
                    let title = match scope {
                        Scope::User => "Update everything for this user",
                        Scope::System => "Update everything on this system",
                    };
                    // Whether this machine will really ask is polkit's answer
                    // and not the scope's: under flatpak's own policy an
                    // update needs no authorization at all. See
                    // `flatpak::will_ask`.
                    let summary = if scope.goes_through_the_helper()
                        && flatpak::will_ask(flatpak::Act::Update)
                    {
                        format!("{count} waiting  ·  authorization required")
                    } else {
                        format!("{count} waiting")
                    };
                    rows.push(head_row(
                        Kind::UpdateAll { scope },
                        title,
                        &summary,
                        "refresh",
                    ));
                }
                // The applications, and then everything they stand on, under
                // a line of type naming it. **What is about to be fetched has
                // to be on the page.** Four of these were left off, so the
                // shelf said seven where eleven were going to be updated and
                // there was no telling where the other four had come from —
                // which is exactly what Discover's Application Support says.
                let (apps, support): (Vec<_>, Vec<_>) =
                    stale.into_iter().partition(|one| one.is_app);
                rows.extend(apps.into_iter().map(|one| self.row_of_installed(one)));
                if !support.is_empty() {
                    rows.push(Row::heading(
                        "Application support",
                        "The runtimes and extensions these stand on",
                    ));
                    rows.extend(support.into_iter().map(|one| self.row_of_support(one)));
                }
                rows
            }
            Shelf::Repositories => {
                let mut rows = vec![head_row(
                    Kind::Add,
                    "Add a repository",
                    "Flathub and the others, or an address of your own",
                    "add",
                )];
                rows.extend(
                    self.machine
                        .listed_remotes()
                        .into_iter()
                        .map(|remote| self.row_of_remote(remote)),
                );
                rows
            }
            Shelf::Section(section) => self
                .catalogue
                .section(section)
                .into_iter()
                .map(|listing| self.row_of_listing(listing))
                .collect(),
        };
        self.sort_rows();
        self.row = self.row.min(self.rows.len().saturating_sub(1));
        // The light must never be left on a line of type.
        if !self
            .rows
            .get(self.row)
            .is_some_and(|row| row.kind.can_be_chosen())
        {
            self.row = self
                .rows
                .iter()
                .position(|row| row.kind.can_be_chosen())
                .unwrap_or(0);
        }
        self.relayout();
        // A re-read that found the same listing must not play the whole entry
        // again: a store that re-read itself after every install would shuffle
        // its own list under whoever was reading it.
        if before != self.rows.len() {
            self.anim.listed_anew();
        }
    }

    /// The Home page: what Flathub says is worth looking at, of the things
    /// this machine can actually install.
    ///
    /// An application on one of those lists that no configured repository
    /// offers is left off rather than shown and refused — the lists are about
    /// Flathub, and this machine may not be reading Flathub at all.
    fn home_rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();

        // Promote the first item from the first collection that this machine's
        // catalogue can actually open. Flathub's order is meaningful (Popular,
        // Trending, New, Updated), while filtering through the catalogue keeps
        // a recommendation from leading to something no configured remote
        // offers. It is left out of the grids below so Home does not repeat the
        // same application immediately under its hero.
        let featured = crate::flathub::Collection::ALL.iter().find_map(|which| {
            self.flathub
                .ids(*which)
                .iter()
                .find_map(|id| self.catalogue.get(id))
        });
        let featured_id = featured.map(|listing| listing.id.as_str());
        if let Some(listing) = featured {
            let mut row = self.row_of_listing(listing);
            row.kind = Kind::Featured;
            rows.push(row);
        }

        for which in crate::flathub::Collection::ALL {
            let found: Vec<Row> = self
                .flathub
                .ids(which)
                .iter()
                .filter_map(|id| self.catalogue.get(id))
                .filter(|listing| Some(listing.id.as_str()) != featured_id)
                .map(|listing| self.row_of_listing(listing))
                .collect();
            if found.is_empty() {
                continue;
            }
            rows.push(Row::heading(which.title(), which.note()));
            rows.extend(found);
        }
        rows
    }

    /// Put the applications on this shelf in the order that has been asked
    /// for, leaving everything that is not one where it is.
    ///
    /// Only the run of application rows at the end moves. A head row acts on
    /// the whole shelf and belongs above it, and Home's headings name what is
    /// under them — neither is a thing to sort.
    ///
    /// **The sort is stable, and that is the whole of the tie-break.** Sorting
    /// on nothing but the one key leaves everything it cannot separate in the
    /// order the shelf built: search results keep their ranking under
    /// `Verified`, and the great many applications whose catalogue entry
    /// carries no timestamp at all fall to the foot under `Newest` still in
    /// the order they were read, rather than into an arbitrary one.
    fn sort_rows(&mut self) {
        if self.shelf().takes_order() {
            let order = self.showing_order();
            put_in_order(&mut self.rows, order);
        }
    }

    /// Which orders this shelf can really answer, in the order a menu lists
    /// them.
    pub fn orders_here(&self) -> Vec<Order> {
        let shelf = self.shelf();
        let rated = self.ratings.any();
        ORDERS
            .into_iter()
            .filter(|order| order.can_answer(shelf, rated))
            .collect()
    }

    /// The order really in force here.
    ///
    /// `self.order` is what was last asked for, and it is kept across shelves
    /// because somebody who asked for the newest first meant it about the way
    /// they read a shelf rather than about the one shelf they were on. Until
    /// anybody has asked, the shelf opens in its own order, which on a
    /// category is best rated. See `Order::in_force` for the whole of it.
    pub fn showing_order(&self) -> Order {
        Order::in_force(self.order, self.shelf(), self.ratings.any())
    }

    /// Whether this shelf is showing enough of a list for its order to mean
    /// anything. An empty Search is a shelf with nothing to put in order.
    pub fn can_be_ordered(&self) -> bool {
        self.shelf().takes_order()
            && self.rows.iter().any(|row| row.kind.is_app())
            && self.orders_here().len() > 1
    }

    /// The menu that changes it, and what to do with the answer.
    pub fn order_menu(&mut self, page: &mut Page) {
        if !self.can_be_ordered() {
            return;
        }
        let shelf = self.shelf();
        let orders = self.orders_here();
        let commands: Vec<&str> = orders.iter().map(|order| order.title(shelf)).collect();
        let showing = self.showing_order();
        let marked = orders.iter().position(|order| *order == showing);
        page.menu_marked(Some("Sort by"), &commands, marked.unwrap_or(usize::MAX));
        self.opened_modal = true;
    }

    /// Take an answer from that menu, if one arrived.
    pub fn take_choice(&mut self, page: &mut Page) {
        let Some(chose) = page.chose() else {
            return;
        };
        let Some(order) = self.orders_here().get(chose).copied() else {
            return;
        };
        // Recorded even where it changes nothing on this shelf, because it is
        // what every shelf after this one is read in: asking a category for
        // the rating it already opened in is still having asked for it.
        let already = order == self.showing_order();
        self.order = Some(order);
        if already {
            return;
        }
        // The light is on a row, and after a sort that row is somewhere else.
        // Putting it back at the top is the honest answer: a listing reordered
        // under a light that stayed put reads as the light having jumped.
        self.row = 0;
        self.top = 0;
        self.stale = true;
        self.rebuild();
        self.anim.listed_anew();
    }

    fn row_of_listing(&self, listing: &crate::catalogue::Listing) -> Row {
        let installed = self.machine.installed_app(&listing.id);
        let rated = self.ratings.of(&listing.id);
        Row {
            id: listing.id.clone(),
            name: listing.name.clone(),
            summary: listing.summary.clone(),
            developer: listing.developer.clone(),
            icon: listing.icon.clone(),
            icon_url: listing.icon_remote.clone(),
            screenshot: listing.screenshots.first().cloned(),
            kind: Kind::App,
            installed: installed.is_some(),
            updatable: installed.is_some_and(|one| one.updatable),
            verified: listing.verified,
            released: listing.released,
            rating: rated.map_or(0.0, |one| one.score()),
            reviews: rated.map_or(0, |one| one.reviews()),
            size: installed.map_or(0, |one| one.size),
            glyph: "launch",
        }
    }

    fn row_of_installed(&self, one: &flatpak::Installed) -> Row {
        let listing = self.catalogue.get(&one.id);
        let rated = self.ratings.of(&one.id);
        Row {
            id: one.id.clone(),
            name: one.name.clone(),
            summary: match &one.eol {
                Some(said) => said.clone(),
                None => listing
                    .map(|listing| listing.summary.clone())
                    .unwrap_or_else(|| flatpak::size(one.size)),
            },
            developer: listing
                .map(|listing| listing.developer.clone())
                .unwrap_or_default(),
            icon: listing.and_then(|listing| listing.icon.clone()),
            icon_url: listing.and_then(|listing| listing.icon_remote.clone()),
            screenshot: listing.and_then(|listing| listing.screenshots.first().cloned()),
            kind: Kind::App,
            installed: true,
            updatable: one.updatable,
            verified: listing.is_some_and(|listing| listing.verified),
            released: listing.map_or(0, |listing| listing.released),
            rating: rated.map_or(0.0, |one| one.score()),
            reviews: rated.map_or(0, |one| one.reviews()),
            size: one.size,
            glyph: "launch",
        }
    }

    /// A runtime, an SDK or an extension on the updates shelf.
    ///
    /// Nothing in any catalogue describes one, so everything on the row comes
    /// off the disk: what it calls itself, what it is going to cost, and which
    /// installation it is in.
    fn row_of_support(&self, one: &flatpak::Installed) -> Row {
        Row {
            id: one.id.clone(),
            name: if one.name.is_empty() {
                one.id.clone()
            } else {
                one.name.clone()
            },
            summary: match one.branch.as_str() {
                "" => flatpak::size(one.size),
                branch => format!("{branch}  ·  {}", flatpak::size(one.size)),
            },
            developer: String::new(),
            icon: None,
            icon_url: None,
            screenshot: None,
            kind: Kind::Support { scope: one.scope },
            installed: true,
            updatable: one.updatable,
            verified: false,
            released: 0,
            rating: 0.0,
            reviews: 0,
            size: one.size,
            glyph: "file-drive",
        }
    }

    fn row_of_remote(&self, remote: &flatpak::Remote) -> Row {
        let offers = self.catalogue.count_from(&remote.name);
        let installed = self.machine.installed_from(&remote.name, remote.scope);
        let mut said = Vec::new();
        said.push(remote.scope.title().to_string());
        if remote.disabled {
            said.push("switched off".into());
        } else if offers > 0 {
            said.push(format!("{offers} on offer"));
        }
        if installed > 0 {
            said.push(format!("{installed} installed from it"));
        }
        Row {
            id: remote.name.clone(),
            name: remote.shown().to_string(),
            summary: said.join("  ·  "),
            developer: String::new(),
            icon: None,
            icon_url: None,
            screenshot: None,
            kind: Kind::Repo {
                scope: remote.scope,
                disabled: remote.disabled,
            },
            installed: false,
            updatable: false,
            verified: false,
            released: 0,
            rating: 0.0,
            reviews: 0,
            size: 0,
            glyph: if remote.disabled {
                "do-not-disturb"
            } else {
                "file-drive"
            },
        }
    }

    /// Lay the rows out into lines of the width there is room for.
    ///
    /// A heading and a head row take a line to themselves; everything else
    /// flows across the grid. Worked out here rather than while drawing,
    /// because moving has to know the same shape drawing does — otherwise Down
    /// and what is under the light disagree.
    fn relayout(&mut self) {
        self.lines = lay_out(&self.rows, self.columns);
        self.settle_the_window();
    }

    /// Which line the light is on, and how far across it.
    pub fn place(&self) -> (usize, usize) {
        place_in(&self.lines, self.row)
    }

    /// Move to a line, keeping as much of the column as that line has.
    ///
    /// A line of type is stepped over rather than landed on: it names what is
    /// under it and there is nothing on it to press.
    fn go_to_line(&mut self, at: usize, column: usize) -> bool {
        let Some(line) = self.lines.get(at) else {
            return false;
        };
        if line.kind == LineKind::Heading {
            return false;
        }
        let Some(row) = line.rows.get(column.min(line.rows.len().saturating_sub(1))) else {
            return false;
        };
        self.row = *row;
        self.settle_the_window();
        true
    }

    /// The next line in a direction that the light can rest on.
    fn line_towards(&self, from: usize, way: isize) -> Option<usize> {
        line_towards_in(&self.lines, from, way)
    }

    /// Keep the chosen line on the screen, and the heading above it with it.
    ///
    /// Worked out in points, not in lines. How many lines fit depends on which
    /// lines they are — a hero is two cards tall and a heading half of one —
    /// so on a page like Home the count depends on where the top is, and
    /// a window settled against that count is settled against a number its own
    /// answer moves. Reaching a card in Home's second section set a top that
    /// had room for a seventh line, which had room to come back up, which had
    /// room to go down again: the listing rocked between three places for as
    /// long as it was left there. Points hold still.
    fn settle_the_window(&mut self) {
        let (line, _) = self.place();
        // A card whose heading is directly above it brings the heading along:
        // a grid scrolled so that the first row of a section is at the very
        // top, with the name of the section just off it, says nothing.
        let with_heading = line
            .checked_sub(1)
            .filter(|above| {
                self.lines
                    .get(*above)
                    .is_some_and(|one| one.kind == LineKind::Heading)
            })
            .unwrap_or(line);
        if with_heading < self.top {
            self.top = with_heading;
        }
        // Far enough down to have the whole of the chosen line, and never
        // further down than the end of the listing. Both ends are read off the
        // geometry alone, so settling a second time settles in the same place.
        let showing = self.top_showing(line);
        let end = self.top_showing(self.lines.len().saturating_sub(1));
        self.top = self.top.clamp(showing, end.max(showing));
    }

    /// The first line that can be drawn at the top with `line` whole below it.
    ///
    /// The whole of it: the peek of air held back at the top of the listing is
    /// what leaves a slice of the next line showing at the foot, and a card
    /// only half on the page is a cue that the list runs on rather than
    /// somewhere the light can rest.
    fn top_showing(&self, line: usize) -> usize {
        let foot = self.line_foot(line);
        let mut top = 0;
        while top < line && foot > self.scroll_at(top) + self.viewport {
            top += 1;
        }
        top
    }

    /// Where a line ends, counting the air under it.
    fn line_foot(&self, line: usize) -> f32 {
        self.line_tops.get(line + 1).copied().unwrap_or(self.deep)
    }

    /// The listing this store would act on, if there is one.
    pub fn chosen(&self) -> Option<&Row> {
        self.rows.get(self.row)
    }

    /// The application a detail page is about, where there is one installed.
    pub fn installed(&self, id: &str) -> Option<&flatpak::Installed> {
        self.machine.installed_app(id)
    }

    /// What an application may reach, worked out once and kept until something
    /// changes it.
    pub fn sandbox_of(&mut self, id: &str) -> Option<&Sandbox> {
        if self.sandbox.as_ref().is_none_or(|(kept, _)| kept != id) {
            let metadata = self.machine.installed_app(id)?.metadata.clone();
            self.sandbox = Some((id.to_string(), Sandbox::read(&metadata, id)));
        }
        self.sandbox.as_ref().map(|(_, sandbox)| sandbox)
    }

    /// The tabs a detail page offers, which is not the same set for every
    /// application: nothing is shown that would open on an empty page.
    pub fn tabs(&self, id: &str) -> Vec<Tab> {
        let mut tabs = vec![Tab::About];
        if self
            .catalogue
            .get(id)
            .is_some_and(|listing| !listing.releases.is_empty())
        {
            tabs.push(Tab::Changes);
        }
        if self.machine.installed_app(id).is_some() {
            tabs.push(Tab::Permissions);
        }
        if self
            .catalogue
            .get(id)
            .is_some_and(|listing| !listing.links.is_empty())
        {
            tabs.push(Tab::Links);
        }
        tabs
    }

    pub fn tab(&self, id: &str) -> Tab {
        let tabs = self.tabs(id);
        tabs.get(self.tab).copied().unwrap_or(Tab::About)
    }

    /// The permission switches a page shows, in the order it shows them.
    pub fn switches(&self) -> Vec<&'static Toggle> {
        TOGGLES.iter().collect()
    }

    /// The links a page offers, in the order it offers them.
    pub fn links(&self, id: &str) -> Vec<(Link, String)> {
        let Some(listing) = self.catalogue.get(id) else {
            return Vec::new();
        };
        Link::ALL
            .into_iter()
            .filter_map(|kind| listing.link(kind).map(|url| (kind, url.to_string())))
            .collect()
    }

    /// Whether a repository name already exists in one installation.
    /// The add page always targets the user installation, so a system remote
    /// with the same name must not disable the authorization-free user copy.
    pub fn repository_is_here(&self, name: &str, scope: Scope) -> bool {
        self.machine
            .remotes
            .iter()
            .any(|remote| remote.name == name && remote.scope == scope)
    }

    /// What the buttons on a detail page are, in the order they are drawn.
    ///
    /// Only what can be *done* to the application. The way out is not one of
    /// them: Back is drawn once, in the legend along the foot of every page,
    /// where it can be pressed with a pointer as readily as with the key it
    /// pictures. A row that carried it as well spent a control saying what the
    /// corner already says, and this row can come out empty.
    pub fn detail_buttons(&self, id: &str) -> Vec<Button> {
        if let Some(running) = &self.running {
            return if running.job.belongs_to_app(id) {
                vec![Button::Stop]
            } else {
                // A second mutation would be silently refused by the worker.
                // Do not offer an action that cannot happen while the footer
                // is already explaining what the store is doing.
                Vec::new()
            };
        }
        let installed = self.machine.installed_app(id);
        let listed = self.catalogue.get(id);
        let mut buttons = Vec::new();
        match installed {
            Some(one) => {
                if one.updatable {
                    buttons.push(Button::Update);
                }
                buttons.push(Button::Open);
                buttons.push(Button::Remove);
            }
            None if listed.is_some() => buttons.push(Button::Install),
            None => {}
        }
        buttons
    }

    /// What the buttons on a repository page are.
    pub fn repository_buttons(&self, name: &str, scope: Scope) -> Vec<Button> {
        if let Some(running) = &self.running {
            return if running.job.belongs_to_repository(name, scope) {
                vec![Button::Stop]
            } else {
                Vec::new()
            };
        }
        let Some(remote) = self.machine.remote(name, scope) else {
            return Vec::new();
        };
        vec![
            if remote.disabled {
                Button::RepoOn
            } else {
                Button::RepoOff
            },
            Button::RepoRefresh,
            Button::RepoForget,
        ]
    }

    /// Whether this is the browse-page row that owns the current job.
    pub fn head_is_running(&self, kind: &Kind) -> bool {
        self.running.as_ref().is_some_and(|running| {
            matches!(
                (kind, &running.job),
                (
                    Kind::UpdateAll { scope },
                    Job::UpdateAll { scope: running_scope }
                ) if scope == running_scope
            ) || matches!((kind, &running.job), (Kind::Trim, Job::Trim { .. }))
        })
    }

    pub fn act(&mut self, page: &mut Page, action: Action) {
        self.opened_modal = false;
        if self.opening() {
            return;
        }
        match self.screen.clone() {
            Screen::Browse => self.act_on_browse(page, action),
            Screen::Detail { id } => self.act_on_detail(page, action, &id),
            Screen::Repository { name, scope } => {
                self.act_on_row_of_buttons(page, action, &self.repository_buttons(&name, scope))
            }
            Screen::AddRepository => self.act_on_adding(page, action),
        }
    }

    fn act_on_browse(&mut self, page: &mut Page, action: Action) {
        match (self.column, action) {
            // The order the listing is in is a fact about the shelf rather
            // than about the row the light happens to be on, so it is offered
            // from every column of a shelf that has one.
            (_, Action::Menu) => self.order_menu(page),
            (Column::Shelves, Action::Up) => {
                if self.shelf > 0 {
                    self.shelf -= 1;
                    self.after_shelf(page);
                }
            }
            (Column::Shelves, Action::Down) => {
                if self.shelf + 1 < shelves().len() {
                    self.shelf += 1;
                    self.after_shelf(page);
                }
            }
            (Column::Shelves, Action::Right | Action::Accept) => {
                // Search opens on its field. Everything else opens on its
                // first row, and a shelf with nothing on it opens on nothing.
                if self.shelf() == Shelf::Search {
                    self.stand_in(Column::Field);
                    self.move_sound(page);
                } else if !self.rows.is_empty() {
                    self.stand_in(Column::Listing);
                    self.move_sound(page);
                }
            }
            // The field. Letters reach it through `take_typing`; these are the
            // presses that are not letters.
            (Column::Field, Action::Down) => {
                if !self.rows.is_empty() {
                    self.stand_in(Column::Listing);
                    self.row = 0;
                    self.top = 0;
                    self.move_sound(page);
                }
            }
            (Column::Field, Action::Accept | Action::Submit) => {
                if !self.rows.is_empty() {
                    self.stand_in(Column::Listing);
                    self.row = 0;
                    self.top = 0;
                    self.press_sound(page);
                }
            }
            (Column::Field, Action::Left | Action::Back | Action::Up) => {
                self.stand_in(Column::Shelves);
                page.play(Sound::Back);
            }
            // Up off the top line of a search answers is the field again,
            // which is where the light came from.
            (Column::Listing, Action::Up)
                if self.shelf() == Shelf::Search && self.place().0 == 0 =>
            {
                self.stand_in(Column::Field);
                page.play(Sound::Back);
            }
            (Column::Listing, Action::Up) => self.step_line(page, -1),
            (Column::Listing, Action::Down) => self.step_line(page, 1),
            (Column::Listing, Action::Right) => {
                self.step_across(page, 1);
            }
            (Column::Listing, Action::Previous) => self.page_by(page, -1),
            (Column::Listing, Action::Next) => self.page_by(page, 1),
            (Column::Listing, Action::Left) => {
                // Left walks back across the line first, and leaves for the
                // shelves only from its near edge.
                if !self.step_across(page, -1) {
                    self.column = self.left_of_the_listing();
                    page.play(Sound::Back);
                }
            }
            (Column::Listing, Action::Accept) => self.open_row(page),
            (Column::Listing, Action::Back) => {
                self.column = self.left_of_the_listing();
                page.play(Sound::Back);
            }
            _ => {}
        }
    }

    /// One line up or down, keeping the place across the line.
    fn step_line(&mut self, page: &mut Page, way: isize) {
        let (line, column) = self.place();
        let Some(next) = self.line_towards(line, way) else {
            return;
        };
        if self.go_to_line(next, column) {
            self.move_sound(page);
        }
    }

    /// One card across the line. Answers whether there was one to move to.
    fn step_across(&mut self, page: &mut Page, way: isize) -> bool {
        let (line, column) = self.place();
        let wanted = column as isize + way;
        let width = self
            .lines
            .get(line)
            .map(|one| one.rows.len() as isize)
            .unwrap_or(0);
        if wanted < 0 || wanted >= width {
            return false;
        }
        if self.go_to_line(line, wanted as usize) {
            self.move_sound(page);
            return true;
        }
        false
    }

    /// A screenful at a time.
    fn page_by(&mut self, page: &mut Page, way: isize) {
        let (line, column) = self.place();
        let room = self.room.max(1) as isize;
        let last = self.lines.len().saturating_sub(1) as isize;
        let mut wanted = (line as isize + way * room).clamp(0, last.max(0)) as usize;
        // Land on something rather than on a line of type; where there is
        // nothing that way, land on whatever is nearest in that direction.
        while self
            .lines
            .get(wanted)
            .is_some_and(|one| one.kind == LineKind::Heading)
        {
            match self.line_towards(wanted, way) {
                Some(next) => wanted = next,
                None => match self.line_towards(wanted, -way) {
                    Some(next) => {
                        wanted = next;
                        break;
                    }
                    None => return,
                },
            }
        }
        if wanted != line && self.go_to_line(wanted, column) {
            self.move_sound(page);
        }
    }

    /// What the light steps back to when it leaves the listing.
    ///
    /// The field on the Search shelf, because that is what it came in
    /// through and because leaving straight for the shelves would put an
    /// on-screen keyboard away and give nothing back for it.
    fn left_of_the_listing(&self) -> Column {
        match self.shelf() {
            Shelf::Search => Column::Field,
            _ => Column::Shelves,
        }
    }

    fn after_shelf(&mut self, page: &mut Page) {
        self.column = Column::Shelves;
        self.settle_the_shelves();
        self.row = 0;
        self.top = 0;
        self.stale = true;
        self.anim.listed_anew();
        self.move_sound(page);
    }

    fn move_sound(&mut self, page: &mut Page) {
        page.play(Sound::Move);
    }

    fn press_sound(&mut self, page: &mut Page) {
        page.play(Sound::Press);
        self.anim.pressed();
    }

    /// Open whatever the light is on, which is not always an application.
    fn open_row(&mut self, page: &mut Page) {
        let Some(row) = self.chosen().cloned() else {
            return;
        };
        let owns_job = self.head_is_running(&row.kind);
        self.press_sound(page);
        self.trouble = None;
        self.note = None;
        match row.kind {
            Kind::App | Kind::Featured => {
                self.screen = Screen::Detail { id: row.id.clone() };
                self.band = Band::Buttons;
                self.button = 0;
                self.tab = 0;
                self.content = 0;
                self.content_top = 0;
                self.shot = 0;
                self.out_of_the_card(&row.id);
                self.weigh(&row.id);
            }
            Kind::Repo { scope, .. } => {
                self.screen = Screen::Repository {
                    name: row.id.clone(),
                    scope,
                };
                self.button = 0;
                self.band = Band::Buttons;
                self.out_of_the_card(&row.id);
            }
            Kind::Add if self.running.is_some() => {
                self.note = Some("Finish the current operation first.".into())
            }
            Kind::Add => {
                self.screen = Screen::AddRepository;
                self.band = Band::Content;
                self.content = 0;
                self.out_of_the_card(&row.id);
            }
            // While something is running, the row that started it stops it.
            // There is nowhere else on a browsing page to put a stop, and a
            // job somebody cannot stop is a job they have to wait out.
            Kind::Heading => {}
            // Nothing describes a runtime, so there is no page to open. What
            // a press on one can honestly do is the one thing somebody would
            // want of it: fetch it.
            Kind::Support { .. } if self.running.is_some() => {
                self.note = Some("Finish the current operation first.".into())
            }
            Kind::Support { scope } => {
                let job = self.machine.support(&row.id, scope).map(|one| Job::Update {
                    scope: one.scope,
                    reference: one.reference.clone(),
                });
                if let Some(job) = job {
                    self.start(job);
                }
            }
            Kind::UpdateAll { .. } | Kind::Trim if owns_job => self.stop(),
            Kind::UpdateAll { .. } | Kind::Trim if self.running.is_some() => {
                self.note = Some("Finish the current operation first.".into())
            }
            Kind::UpdateAll { scope } => self.start(Job::UpdateAll { scope }),
            Kind::Trim => self.start(Job::Trim { scope: Scope::User }),
        }
    }

    /// Whether the worker is out working out what an install would cost.
    pub fn weighing(&self, id: &str) -> bool {
        self.weighing
            .as_ref()
            .is_some_and(|about| about.split('/').nth(1) == Some(id))
    }

    /// Ask what an install would cost, if it has not been asked already.
    ///
    /// The answer comes from resolving a real transaction against the remote,
    /// which is the only way to learn the number that counts every runtime and
    /// extension the application needs and does not already have.
    pub fn weigh(&mut self, id: &str) {
        if self.weights.contains_key(id)
            || self.weighing.is_some()
            || self.running.is_some()
            || self.machine.installed_app(id).is_some()
        {
            return;
        }
        let Some(listing) = self.catalogue.get(id) else {
            return;
        };
        let job = Job::Weigh {
            scope: self.machine.install_scope(&listing.remote),
            remote: listing.remote.clone(),
            reference: listing.reference.clone(),
        };
        self.weighing = Some(job.about());
        self.worker.send(job);
    }

    fn act_on_detail(&mut self, page: &mut Page, action: Action, id: &str) {
        let buttons = self.detail_buttons(id);
        let tabs = self.tabs(id);
        self.settle_the_buttons(&buttons);
        if action == Action::Back {
            self.leave(page);
            return;
        }
        match crossing(
            self.band,
            action,
            self.one_row,
            self.button,
            buttons.len(),
            self.tab,
        ) {
            Some(Crossing::To(band)) => {
                self.cross_to(page, band);
                return;
            }
            Some(Crossing::ToLastControl) => {
                self.button = buttons.len() - 1;
                self.cross_to(page, Band::Buttons);
                return;
            }
            None => {}
        }
        match self.band {
            Band::Buttons => self.act_on_row_of_buttons(page, action, &buttons),
            Band::Content => self.act_on_content(page, action, id),
            Band::Tabs => match action {
                Action::Left if self.tab > 0 => {
                    self.tab -= 1;
                    self.after_tab(page);
                }
                Action::Right if self.tab + 1 < tabs.len() => {
                    self.tab += 1;
                    self.after_tab(page);
                }
                _ => {}
            },
        }
    }

    fn after_tab(&mut self, page: &mut Page) {
        self.content = 0;
        self.content_top = 0;
        self.anim.listed_anew();
        self.move_sound(page);
    }

    /// Keep the light off a control that is not there.
    ///
    /// The row of buttons is what can be done to a thing, so it shrinks to
    /// nothing while a job is running elsewhere and grows back when it is
    /// over. Nothing may point past the end of it, and the top band cannot
    /// hold the light while it is empty.
    pub fn settle_the_buttons(&mut self, buttons: &[Button]) {
        self.button = self.button.min(buttons.len().saturating_sub(1));
        if buttons.is_empty() && self.band == Band::Buttons {
            self.band = Band::Tabs;
        }
    }

    fn act_on_content(&mut self, page: &mut Page, action: Action, id: &str) {
        let tab = self.tab(id);
        let deep = self.content_depth();
        let reset_available =
            tab != Tab::Permissions || self.sandbox_of(id).is_some_and(Sandbox::touched);
        match action {
            Action::Up if self.content == 0 || (self.content == 1 && !reset_available) => {
                let back = back_up(
                    self.one_row,
                    self.came_down_from,
                    self.detail_buttons(id).len(),
                );
                self.cross_to(page, back)
            }
            Action::Up => {
                self.content -= 1;
                self.settle_the_content();
                self.move_sound(page);
            }
            Action::Down if self.content + 1 < deep => {
                self.content += 1;
                self.settle_the_content();
                self.move_sound(page);
            }
            Action::Left | Action::Right if tab == Tab::About => {
                let shots = self
                    .catalogue
                    .get(id)
                    .map(|listing| listing.screenshots.len())
                    .unwrap_or(0);
                if shots > 1 {
                    self.shot = if action == Action::Right {
                        (self.shot + 1) % shots
                    } else {
                        (self.shot + shots - 1) % shots
                    };
                    self.anim.crossed();
                    self.move_sound(page);
                }
            }
            Action::Accept => self.press_in_content(page, id),
            _ => {}
        }
    }

    /// How many steps the content band has, which is what Up and Down walk.
    ///
    /// Drawing works this out, because only drawing knows how many lines a
    /// description came to at this window's width. Reading it back is how
    /// moving stays inside what is really there.
    pub fn content_depth(&self) -> usize {
        self.content_deep
    }

    /// Written down as the two are laid out.
    ///
    /// **A direction has to mean what the page looks like it means.** The
    /// controls and the tabs are two bands of the page's own model, and while
    /// they are drawn on one line Down through them would walk the light
    /// sideways — the fault every merged row invites. So on one line they are
    /// walked as one row: Right off the end of the controls lands on the tabs,
    /// Left off the front of the tabs lands back on the controls, and Down
    /// from either is the reading below. Wrapped onto two lines, which is what
    /// a narrow window and four controls come to, they go back to being two
    /// bands stacked.
    pub fn one_row_is(&mut self, one: bool) {
        self.one_row = one;
    }

    /// Written down as each bar is drawn, and as nothing where one was not.
    pub fn listing_bar_is(&mut self, track: [f32; 4], thumb: f32) {
        self.listing_track = (track, thumb);
    }

    pub fn content_bar_is(&mut self, track: [f32; 4], thumb: f32) {
        self.content_track = (track, thumb);
    }

    /// How far down a bar a pointer has taken its thumb, where a hand is on
    /// one at all. See [`share_along`].
    pub fn listing_pulled(&self, at: [f32; 2]) -> Option<f32> {
        let (track, thumb) = self.listing_track;
        (track[3] > 0.0).then(|| share_along(track, thumb, at[1]))
    }

    pub fn content_pulled(&self, at: [f32; 2]) -> Option<f32> {
        let (track, thumb) = self.content_track;
        (track[3] > 0.0).then(|| share_along(track, thumb, at[1]))
    }

    /// Written down as the content band is drawn.
    pub fn content_is(&mut self, deep: usize, room: usize) {
        self.content_deep = deep;
        self.content_room = room.max(1);
        self.content = self.content.min(deep.saturating_sub(1));
        self.settle_the_content();
    }

    /// The same two numbers for the band of a detail page that scrolls.
    pub fn content_bar(&self) -> Option<(f32, f32)> {
        let deep = self.content_deep;
        let room = self.content_room.max(1);
        let over = deep.saturating_sub(room);
        (over > 0).then(|| {
            (
                (self.content_top as f32 / over as f32).clamp(0.0, 1.0),
                (room as f32 / deep as f32).clamp(0.0, 1.0),
            )
        })
    }

    /// And the same drag, on that band. The light leads here too: every row
    /// of a detail page's content is a step, and `settle_the_content` is what
    /// keeps the chosen one showing.
    pub fn pull_content_to(&mut self, share: f32) {
        let deep = self.content_deep;
        let over = deep.saturating_sub(self.content_room.max(1));
        if over == 0 {
            return;
        }
        self.band = Band::Content;
        self.content =
            ((share.clamp(0.0, 1.0) * over as f32).round() as usize).min(deep.saturating_sub(1));
        self.content_top = self.content;
        self.settle_the_content();
    }

    /// Keep the chosen content row on the screen.
    fn settle_the_content(&mut self) {
        let room = self.content_room.max(1);
        if self.content < self.content_top {
            self.content_top = self.content;
        } else if self.content >= self.content_top + room {
            self.content_top = self.content + 1 - room;
        }
        let most = self.content_deep.saturating_sub(room);
        self.content_top = self.content_top.min(most);
    }

    fn press_in_content(&mut self, page: &mut Page, id: &str) {
        match self.tab(id) {
            Tab::Permissions => {
                let switches = self.switches();
                if self.content == 0 {
                    if !self.sandbox_of(id).is_some_and(Sandbox::touched) {
                        return;
                    }
                    self.press_sound(page);
                    match sandbox::forget(id) {
                        Ok(()) => {
                            self.note =
                                Some("Everything is back to what the application asked for.".into())
                        }
                        Err(why) => self.trouble = Some(why),
                    }
                    self.sandbox = None;
                    return;
                }
                let Some(toggle) = switches.get(self.content - 1).copied() else {
                    return;
                };
                self.press_sound(page);
                let Some(sandbox) = self.sandbox_of(id) else {
                    return;
                };
                let on = !sandbox.standing(toggle).on();
                let answer = sandbox::set(sandbox, id, toggle, on);
                match answer {
                    Ok(()) => {
                        self.note = Some(format!(
                            "{} is {} for {}.",
                            toggle.title,
                            if on { "on" } else { "off" },
                            self.name_of(id)
                        ));
                        self.trouble = None;
                    }
                    Err(why) => self.trouble = Some(why),
                }
                // Read back rather than believed: what was written is what the
                // application will actually run with.
                self.sandbox = None;
            }
            Tab::Links => {
                let links = self.links(id);
                if let Some((_, url)) = links.get(self.content) {
                    self.press_sound(page);
                    open_out_there(url);
                }
            }
            _ => {}
        }
    }

    fn cross_to(&mut self, page: &mut Page, band: Band) {
        if self.band == band {
            return;
        }
        if band == Band::Content {
            // Written down before the move, because it is where the light is
            // leaving from and not where it is going.
            self.came_down_from = self.band;
            self.content = 0;
            self.content_top = 0;
        }
        self.band = band;
        self.move_sound(page);
    }

    fn act_on_row_of_buttons(&mut self, page: &mut Page, action: Action, buttons: &[Button]) {
        match action {
            Action::Left => {
                if self.button > 0 {
                    self.button -= 1;
                    self.move_sound(page);
                }
            }
            Action::Right => {
                if self.button + 1 < buttons.len() {
                    self.button += 1;
                    self.move_sound(page);
                }
            }
            Action::Back => self.leave(page),
            Action::Accept => {
                // A row with nothing in it answers nothing. There used to be a
                // Back here to fall back on, and falling back on it meant a
                // press on a page with no controls left the page.
                if let Some(button) = buttons.get(self.button).copied() {
                    self.press(page, button);
                }
            }
            _ => {}
        }
    }

    fn act_on_adding(&mut self, page: &mut Page, action: Action) {
        let deep = flatpak::KNOWN.len() + 1;
        self.content_top = 0;
        // Writing an address is a place to be got out of before the screen is,
        // and moving off the row is one of the ways out: an address row that
        // went on taking letters from three rows away is the fault this whole
        // control was made a control to be rid of.
        if self.adding.typing && !matches!(action, Action::Accept | Action::Submit) {
            self.adding.typing = false;
            if matches!(action, Action::Back) {
                page.play(Sound::Back);
                return;
            }
        }
        match action {
            Action::Back | Action::Left => self.leave(page),
            Action::Up if self.content > 0 => {
                self.content -= 1;
                self.move_sound(page);
            }
            Action::Down if self.content + 1 < deep => {
                self.content += 1;
                self.move_sound(page);
            }
            Action::Accept | Action::Submit => match flatpak::KNOWN.get(self.content) {
                Some(known) if self.repository_is_here(known.name, Scope::User) => {
                    self.note = Some(format!("{} is already on this machine.", known.title));
                    self.trouble = None;
                }
                Some(known) => {
                    self.press_sound(page);
                    self.add_repository(known.name.to_string(), known.url.to_string())
                }
                // The address row. A press on it is what begins writing one;
                // a press while writing is what tries it.
                None if !self.adding.typing => {
                    self.press_sound(page);
                    self.adding.typing = true;
                    self.trouble = None;
                }
                None if self.adding.ready() => {
                    self.press_sound(page);
                    self.adding.typing = false;
                    let name = self.adding.name.trim().to_string();
                    let url = self.adding.url.trim().to_string();
                    self.add_repository(name, url);
                }
                None => {
                    self.press_sound(page);
                    self.trouble =
                        Some("An address has to start with https:// and name a repository.".into())
                }
            },
            _ => {}
        }
    }

    fn add_repository(&mut self, name: String, url: String) {
        if self.running.is_some() {
            self.note = Some("Finish the current operation before adding a repository.".into());
            return;
        }
        if self.repository_is_here(&name, Scope::User) {
            self.note = Some(format!("{name} is already on this machine."));
            self.trouble = None;
            return;
        }
        self.start(Job::Repository {
            scope: Scope::User,
            job: RepoJob::Add { name, url },
        });
        self.back_into_the_card();
    }

    fn leave(&mut self, page: &mut Page) {
        page.play(Sound::Back);
        match &self.screen {
            Screen::Browse => {}
            Screen::Repository { .. } | Screen::AddRepository | Screen::Detail { .. } => {
                self.trouble = None;
                self.back_into_the_card();
            }
        }
    }

    pub fn press(&mut self, page: &mut Page, button: Button) {
        self.trouble = None;
        self.note = None;
        let screen = self.screen.clone();

        // Destructive controls ask before they alter the machine. The safe
        // answer comes first and has focus, so an accidental double press
        // cannot confirm the action it just opened.
        match (button, &screen) {
            (Button::Remove, Screen::Detail { id }) => {
                let name = self.name_of(id);
                let permission = self
                    .machine
                    .installed_app(id)
                    .is_some_and(|one| one.scope.goes_through_the_helper())
                    && flatpak::will_ask(flatpak::Act::Remove);
                let mut body = format!(
                    "{name} will be removed. Its settings and files in your home folder will stay."
                );
                if permission {
                    body.push_str(" The desktop will ask for authorization.");
                }
                self.confirming = Some(Confirmation::Remove { id: id.clone() });
                self.opened_modal = true;
                page.ask(&format!("Remove {name}?"), &body, &["Keep it", "Remove"]);
                return;
            }
            (Button::RepoForget, Screen::Repository { name, scope }) => {
                let shown = self
                    .machine
                    .remote(name, *scope)
                    .map(|remote| remote.shown().to_string())
                    .unwrap_or_else(|| name.clone());
                let body = format!(
                    "{shown} will no longer offer applications or updates. Applications already installed from it will stay installed."
                );
                self.confirming = Some(Confirmation::Forget {
                    name: name.clone(),
                    scope: *scope,
                });
                self.opened_modal = true;
                page.ask(&format!("Forget {shown}?"), &body, &["Keep it", "Forget"]);
                return;
            }
            _ => {}
        }

        self.press_sound(page);
        match (button, &screen) {
            (Button::Stop, _) => self.stop(),
            (Button::Install, Screen::Detail { id }) => self.begin(Doing::Install, id),
            (Button::Update, Screen::Detail { id }) => self.begin(Doing::Update, id),
            (Button::Open, Screen::Detail { id }) => self.begin(Doing::Open, id),
            (Button::RepoOn | Button::RepoOff, Screen::Repository { name, scope }) => {
                self.start(Job::Repository {
                    scope: *scope,
                    job: RepoJob::Enable {
                        name: name.clone(),
                        on: button == Button::RepoOn,
                    },
                });
            }
            (Button::RepoRefresh, Screen::Repository { name, scope }) => {
                self.start(Job::Repository {
                    scope: *scope,
                    job: RepoJob::Refresh { name: name.clone() },
                });
            }
            _ => {}
        }
    }

    /// Take whether the most recently handled action opened a modal question.
    pub fn take_opened_modal(&mut self) -> bool {
        std::mem::take(&mut self.opened_modal)
    }

    /// Take the answer to a destructive-action dialog, if one arrived.
    pub fn take_answer(&mut self, page: &mut Page) {
        let Some(answer) = page.answered() else {
            return;
        };
        let Some(confirming) = self.confirming.take() else {
            return;
        };
        if answer != 1 {
            return;
        }

        match confirming {
            Confirmation::Remove { id } => self.begin(Doing::Remove, &id),
            Confirmation::Forget { name, scope } => {
                self.start(Job::Repository {
                    scope,
                    job: RepoJob::Forget { name },
                });
                self.back_into_the_card();
            }
        }
    }

    /// Stop whatever is running. What has already been fetched stays fetched,
    /// which is what makes starting again cheap.
    fn stop(&mut self) {
        self.worker.stop();
        if let Some(running) = &mut self.running {
            running.stopping = true;
            running.step = "Stopping".into();
        }
    }

    fn name_of(&self, id: &str) -> String {
        self.catalogue
            .get(id)
            .map(|listing| listing.name.clone())
            .or_else(|| self.machine.installed_app(id).map(|one| one.name.clone()))
            .unwrap_or_else(|| id.to_string())
    }

    fn begin(&mut self, doing: Doing, id: &str) {
        let job = match doing {
            Doing::Install => {
                let Some(listing) = self.catalogue.get(id) else {
                    return;
                };
                Job::Install {
                    scope: self.machine.install_scope(&listing.remote),
                    remote: listing.remote.clone(),
                    reference: listing.reference.clone(),
                }
            }
            Doing::Update => {
                let Some(one) = self.machine.installed_app(id) else {
                    return;
                };
                Job::Update {
                    scope: one.scope,
                    reference: one.reference.clone(),
                }
            }
            Doing::Remove => {
                let Some(one) = self.machine.installed_app(id) else {
                    return;
                };
                Job::Remove {
                    scope: one.scope,
                    reference: one.reference.clone(),
                }
            }
            Doing::Open => {
                let Some(one) = self.machine.installed_app(id) else {
                    return;
                };
                Job::Open {
                    scope: one.scope,
                    id: one.id.clone(),
                }
            }
        };
        self.start(job);
    }

    fn start(&mut self, job: Job) {
        if self.running.is_some() {
            return;
        }
        self.trouble = None;
        let name = match &job {
            Job::Install { reference, .. }
            | Job::Update { reference, .. }
            | Job::Remove { reference, .. } => reference
                .split('/')
                .nth(1)
                .map(|id| self.name_of(id))
                .unwrap_or_default(),
            Job::Open { id, .. } => self.name_of(id),
            Job::Repository { job, .. } => job.name().to_string(),
            Job::UpdateAll { .. } | Job::Trim { .. } | Job::Weigh { .. } => String::new(),
        };

        // A weigh is arithmetic and an open is over at once; neither is worth
        // a bar, and a bar that flashed would only read as a fault.
        if job.shown() {
            // The available controls change to Stop and Back as soon as a
            // page job begins. A destructive action may have been the third
            // button; carrying that index into a two-button row leaves no
            // control lit and makes Accept fall back to Back.
            self.button = 0;
            self.running = Some(Running {
                step: job.doing(),
                job: job.clone(),
                name,
                through: 0.0,
                at: 0,
                of: 1,
                transferred: 0,
                stopping: false,
            });
            self.anim.restarted();
        }
        self.worker.send(job);
    }

    /// Everything typed since the last frame, which two places take.
    pub fn take_typing(&mut self, page: &mut Page) {
        // **Only while the light is standing in the field.** Taking text
        // wherever the Search shelf happened to be open spent Space on a
        // letter instead of on Accept, and told the compositor a field had the
        // cursor while the light was three rows down a grid of answers.
        let taking = match &self.screen {
            Screen::Browse => self.column == Column::Field,
            // Only while the address is being written into, and never while
            // the light is on one of the four repositories above it. Said for
            // the whole screen, this raised an on-screen keyboard over a
            // column where four rows out of five do nothing with a letter.
            Screen::AddRepository => self.adding.typing,
            _ => false,
        };
        if !taking {
            return;
        }
        page.taking_text(true);

        let into_search = self.screen == Screen::Browse;
        let mut changed = false;
        for typed in page.typed() {
            match (&typed, into_search) {
                (Typed::Wrote(text), true) => {
                    self.query.push_str(text);
                    changed = true;
                }
                (Typed::Rubbed, true) => changed |= self.query.pop().is_some(),
                (Typed::Wrote(text), false) => {
                    let mut url = self.adding.url.clone();
                    url.push_str(text);
                    self.adding.typed_url(url);
                }
                (Typed::Rubbed, false) => {
                    let mut url = self.adding.url.clone();
                    url.pop();
                    self.adding.typed_url(url);
                }
            }
        }
        if changed {
            self.row = 0;
            self.top = 0;
            self.stale = true;
            self.anim.listed_anew();
        }
    }

    /// Set the row a pointer landed on, without acting on it.
    /// Move the light to another column of the browsing page.
    ///
    /// **A light is not carried between the field and anything else.** The
    /// field is a bar the width of the listing; a shelf is a row inside a
    /// panel, and a card is a tile in a grid. The three are drawn in three
    /// ranges cut to three different rectangles, so a light springing from the
    /// field to a shelf is a capsule of the field's width crossing the panel's
    /// own edge — which is what "a button becoming glitchy when going back
    /// from the search" was. It is placed instead, which is what
    /// `Light::settle` is for. Between two things cut the same way — a shelf
    /// to a shelf, a card to a card, a shelf to a card — it still glides.
    fn stand_in(&mut self, column: Column) {
        if (self.column == Column::Field) != (column == Column::Field) {
            self.anim.light.settle();
        }
        self.column = column;
    }

    /// The pointer landed on the search field.
    pub fn point_at_field(&mut self) {
        if self.shelf() == Shelf::Search {
            self.stand_in(Column::Field);
        }
    }

    pub fn point_at_row(&mut self, at: usize) {
        if self
            .rows
            .get(at)
            .is_some_and(|row| row.kind.can_be_chosen())
        {
            self.stand_in(Column::Listing);
            self.row = at;
            self.settle_the_window();
        }
    }

    pub fn point_at_shelf(&mut self, at: usize) {
        if at < shelves().len() && at != self.shelf {
            self.shelf = at;
            self.settle_the_shelves();
            self.row = 0;
            self.top = 0;
            self.stale = true;
            self.anim.listed_anew();
        }
        self.stand_in(Column::Shelves);
    }

    /// How far down the listing it stands and how much of it is showing —
    /// what a bar down its edge draws itself from, both from nought to one.
    ///
    /// `None` where the whole of it fits, because there is then nothing for a
    /// bar to say and a full-length thumb saying it is a control that cannot
    /// be moved.
    pub fn listing_bar(&self) -> Option<(f32, f32)> {
        let over = self.deep - self.viewport;
        (over > 1.0 && self.viewport > 0.0).then(|| {
            (
                (self.scroll_target() / over).clamp(0.0, 1.0),
                (self.viewport / self.deep).clamp(0.0, 1.0),
            )
        })
    }

    /// Put the light where a bar dragged to `share` of the way down says.
    ///
    /// **The light is what scrolls this listing.** `settle_the_window` keeps
    /// the chosen row on the page and the list follows it, so a bar moves the
    /// light rather than the view; a page that scrolled away from its own
    /// selection would jump back the instant a direction was pressed.
    ///
    /// The line dragged to is put at the top, and the light goes on it — or
    /// on the first line under it that is not a heading, because a heading
    /// names what is beneath it and there is nothing on it to press.
    pub fn pull_listing_to(&mut self, share: f32) {
        let over = self.deep - self.viewport;
        if over <= 1.0 || self.lines.is_empty() {
            return;
        }
        let want = self.peek + share.clamp(0.0, 1.0) * over;
        let at = self
            .line_tops
            .iter()
            .rposition(|top| *top <= want + 0.5)
            .unwrap_or(0);
        let (_, column) = self.place();
        self.column = Column::Listing;
        for line in at..self.lines.len() {
            if self.go_to_line(line, column) {
                return;
            }
        }
        // Dragged past the last thing that can be landed on, which is a list
        // ending in a heading with nothing under it yet.
        for line in (0..at).rev() {
            if self.go_to_line(line, column) {
                return;
            }
        }
    }

    /// Where the listing is scrolled to, in points down the lines.
    ///
    /// The list is held back from its own top edge by a peek, so that the
    /// line before the first one shows through it and says the list runs on.
    /// At the beginning of a list there is nothing above to show and the peek
    /// is air — held back all the same, because a list whose first line moved
    /// as soon as the second one was reached would be a list that jumped.
    fn scroll_target(&self) -> f32 {
        self.scroll_at(self.top)
    }

    /// Where it would be scrolled to with `top` as its first line.
    fn scroll_at(&self, top: usize) -> f32 {
        self.line_tops.get(top).copied().unwrap_or(0.0) - self.peek
    }

    /// Where the panel of shelves is scrolled to, in points.
    ///
    /// In points rather than in shelves, so that the spring carrying it and
    /// the spring carrying the listing are settling against the same units and
    /// the same "near enough".
    fn shelf_scroll_target(&self) -> f32 {
        self.shelf_top as f32 * self.shelf_height
    }

    /// Keep the chosen shelf inside the panel.
    fn settle_the_shelves(&mut self) {
        let room = self.shelf_room.max(1);
        if self.shelf < self.shelf_top {
            self.shelf_top = self.shelf;
        } else if self.shelf >= self.shelf_top + room {
            self.shelf_top = self.shelf + 1 - room;
        }
        self.shelf_top = self.shelf_top.min(shelves().len().saturating_sub(room));
    }

    /// How many shelves the panel had room for, written down as it was drawn.
    ///
    /// The same bargain the listing's own shape is written down under: moving
    /// has to know what drawing did, or a press of Down scrolls the panel by a
    /// different amount than the one the eye just measured.
    pub fn shelves_are(&mut self, room: usize, height: f32) {
        let room = room.max(1);
        self.shelf_height = height;
        if self.shelf_room != room {
            // A panel that grew shows what was above it too; see the listing's
            // own half of this in `shape_is`.
            let grew = room.saturating_sub(self.shelf_room);
            self.shelf_room = room;
            self.shelf_top = self.shelf_top.saturating_sub(grew);
            self.settle_the_shelves();
        }
    }

    /// The shape the last frame laid out, written down as it drew.
    ///
    /// Moving has to know what drawing did: how wide the grid came out, how
    /// many lines fitted, where each line begins, how tall the listing is on
    /// the page and how deep it runs. A store that guessed any of those would
    /// move the light somewhere other than where the card it landed on is.
    pub fn shape_is(&mut self, shape: Shape) {
        let columns = shape.columns.max(1);
        self.line_tops = shape.tops;
        self.peek = shape.peek;
        self.viewport = shape.viewport;
        self.deep = shape.deep;
        self.room = shape.room.max(1);
        if self.columns != columns {
            self.columns = columns;
            self.relayout();
        }
        // Settled against the shape every frame rather than only when it
        // changes. A window that grew shows what was above it as well as what
        // was below, and the first frame — laid out before anything knows how
        // tall the page is — is a shape like any other.
        self.settle_the_window();
    }
}

/// Lay rows out into lines of the width there is room for.
///
/// A heading and a head row take a line to themselves; everything else flows
/// across the grid. Worked out away from drawing, because moving has to know
/// the same shape drawing does — otherwise Down and what is under the light
/// disagree about where the light went.
fn lay_out(rows: &[Row], columns: usize) -> Vec<Line> {
    let columns = columns.max(1);
    let mut lines: Vec<Line> = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let kind = match &row.kind {
            Kind::Featured => LineKind::Hero,
            Kind::Heading => LineKind::Heading,
            kind if kind.is_wide() => LineKind::Wide,
            _ => LineKind::Cells,
        };
        let carries_on = kind == LineKind::Cells
            && lines
                .last()
                .is_some_and(|line| line.kind == LineKind::Cells && line.rows.len() < columns);
        match carries_on {
            true => {
                if let Some(line) = lines.last_mut() {
                    line.rows.push(index);
                }
            }
            false => lines.push(Line {
                kind,
                rows: vec![index],
            }),
        }
    }
    lines
}

/// Which line a row is on, and how far across it.
fn place_in(lines: &[Line], row: usize) -> (usize, usize) {
    for (at, line) in lines.iter().enumerate() {
        if let Some(column) = line.rows.iter().position(|one| *one == row) {
            return (at, column);
        }
    }
    (0, 0)
}

/// The next line in a direction that the light can rest on.
///
/// A line of type is stepped over rather than landed on: it names what is
/// under it and there is nothing on it to press.
fn line_towards_in(lines: &[Line], from: usize, way: isize) -> Option<usize> {
    let mut at = from as isize;
    loop {
        at += way;
        if at < 0 || at as usize >= lines.len() {
            return None;
        }
        if lines[at as usize].kind != LineKind::Heading {
            return Some(at as usize);
        }
    }
}

/// A row that acts on a whole shelf rather than opening one thing on it.
/// Put the applications in a listing in the order that has been asked for,
/// leaving everything that is not one where it is.
///
/// Only the run of application rows at the end moves. A head row acts on the
/// whole shelf and belongs above it, and Home's headings name what is under
/// them — neither is a thing to sort, and a listing that is not one clean run
/// of applications is left alone entirely rather than shuffled into nonsense.
///
/// **The sort is stable, and that is the whole of the tie-break.** Sorting on
/// nothing but the one key leaves everything it cannot separate in the order
/// the shelf built it: search results keep their ranking under `Verified`, and
/// the great many applications whose catalogue entry carries no timestamp at
/// all fall to the foot under `Newest` still in the order they were read,
/// rather than into an arbitrary one.
fn put_in_order(rows: &mut [Row], order: Order) {
    if order == Order::Best {
        return;
    }
    // **Only the applications, and only where they are one unbroken run.** A
    // shelf is a head row or two, then the applications, and on the updates
    // shelf a line of type and the runtimes under it as well. Neither end is
    // in the order: sorting from the first application to the foot of the
    // page shuffled Application support up into the middle of the
    // applications, and the guard that used to stop that stopped the sort
    // instead — Sort on the updates shelf quietly did nothing.
    let Some(first) = rows.iter().position(|row| row.kind.is_app()) else {
        return;
    };
    let after = rows[first..]
        .iter()
        .position(|row| !row.kind.is_app())
        .map_or(rows.len(), |at| first + at);
    if rows[after..].iter().any(|row| row.kind.is_app()) {
        return;
    }
    let rows = &mut rows[first..after];
    let first = 0;
    match order {
        Order::Best => {}
        // `f32` is not `Ord`, and a rating is never a number that is not one:
        // it is built from counts. Ordering it by the bits of its negation is
        // exact and puts the largest first.
        Order::Rating => rows[first..].sort_by(|one, other| {
            other
                .rating
                .partial_cmp(&one.rating)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        Order::Reviews => rows[first..].sort_by_key(|row| std::cmp::Reverse(row.reviews)),
        Order::Size => rows[first..].sort_by_key(|row| std::cmp::Reverse(row.size)),
        Order::Newest => rows[first..].sort_by_key(|row| std::cmp::Reverse(row.released)),
        Order::Verified => rows[first..].sort_by_key(|row| !row.verified),
    }
}

fn head_row(kind: Kind, name: &str, summary: &str, glyph: &'static str) -> Row {
    Row {
        id: String::new(),
        name: name.to_string(),
        summary: summary.to_string(),
        developer: String::new(),
        icon: None,
        icon_url: None,
        screenshot: None,
        kind,
        installed: false,
        updatable: false,
        verified: false,
        released: 0,
        rating: 0.0,
        reviews: 0,
        size: 0,
        glyph,
    }
}

/// Hand an address to whatever this desktop opens addresses with.
///
/// `xdg-open`, because a store is not a browser and has no business being one.
/// Detached on purpose: nothing here waits for a browser to start, and nothing
/// here is the parent of one when this window closes.
fn open_out_there(url: &str) {
    if !url.starts_with("http") {
        return;
    }
    let answer = std::process::Command::new("xdg-open")
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    if let Err(err) = answer {
        eprintln!("distribumpy: {url}: {err}");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Install,
    Update,
    Remove,
    Open,
    Stop,
    RepoOn,
    RepoOff,
    RepoRefresh,
    RepoForget,
}

impl Button {
    pub fn label(self) -> &'static str {
        match self {
            Button::Install => "Install",
            Button::Update => "Update",
            Button::Remove => "Remove",
            Button::Open => "Open",
            Button::Stop => "Stop",
            Button::RepoOn => "Switch on",
            Button::RepoOff => "Switch off",
            Button::RepoRefresh => "Fetch catalogue",
            Button::RepoForget => "Forget",
        }
    }

    /// Whether pressing this takes something away, which is the one thing a
    /// page has to say before it is pressed rather than after.
    pub fn grave(self) -> bool {
        matches!(self, Button::Remove | Button::RepoForget)
    }
}

enum Doing {
    Install,
    Update,
    Remove,
    Open,
}

/// Read the machine and its catalogue away from the frame loop.
///
/// Flathub's catalogue takes a second and a half to read, which is fifteen
/// dropped frames if it is done where the window is drawn. Nothing about it is
/// urgent — the page says it is reading and comes to life when it is done.
fn read_in_the_background() -> std::sync::mpsc::Receiver<(Machine, Catalogue)> {
    let (voice, answer) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("distribumpy-catalogue".into())
        .spawn(move || {
            let machine = Machine::read();
            let catalogue = Catalogue::read(&machine.catalogue_remotes());
            let _ = voice.send((machine, catalogue));
        })
        .expect("a catalogue thread");
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shelves_begin_with_the_four_that_are_not_categories() {
        let shelves = shelves();
        assert_eq!(
            &shelves[..4],
            &[Shelf::Home, Shelf::Search, Shelf::Updates, Shelf::Installed],
            "the shelves that answer a question moved below the ones that browse"
        );
        assert_eq!(
            shelves.first(),
            Some(&Shelf::Home),
            "the shelf somebody lands on is not the first one"
        );
        assert_eq!(
            shelves.last(),
            Some(&Shelf::Repositories),
            "the shelf that is about the machine rather than about applications \
             was mixed in among the ones that are"
        );
        assert_eq!(
            shelves.len(),
            5 + Section::ALL.len(),
            "a section was added without a shelf, or the other way about"
        );
    }

    /// One application row with nothing said about it.
    fn blank(name: &str) -> Row {
        Row {
            id: format!("org.example.{name}"),
            name: name.to_string(),
            summary: String::new(),
            developer: String::new(),
            icon: None,
            icon_url: None,
            screenshot: None,
            kind: Kind::App,
            installed: false,
            updatable: false,
            verified: false,
            released: 0,
            rating: 0.0,
            reviews: 0,
            size: 0,
            glyph: "launch",
        }
    }

    /// One application row, dated and vouched for or not.
    fn app(name: &str, released: i64, verified: bool) -> Row {
        Row {
            verified,
            released,
            ..blank(name)
        }
    }

    /// One application row, with what people said about it.
    fn rated(name: &str, rating: f32, reviews: u32) -> Row {
        Row {
            rating,
            reviews,
            ..blank(name)
        }
    }

    /// One application row, with what it takes up.
    fn sized(name: &str, size: u64) -> Row {
        Row {
            size,
            ..blank(name)
        }
    }

    fn names(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|row| row.name.as_str()).collect()
    }

    /// A head row and four applications, built the way a shelf builds them:
    /// by name, which is the order `Order::Best` means everywhere but Search.
    fn a_shelf() -> Vec<Row> {
        vec![
            head_row(Kind::Add, "Add", "a head row", "setting-connect"),
            app("Alma", 300, false),
            app("Bruno", 100, true),
            app("Clara", 0, true),
            app("Dora", 200, false),
        ]
    }

    #[test]
    fn the_order_a_shelf_built_is_left_alone() {
        let mut rows = a_shelf();
        put_in_order(&mut rows, Order::Best);
        assert_eq!(names(&rows), ["Add", "Alma", "Bruno", "Clara", "Dora"]);
    }

    #[test]
    fn the_newest_comes_first_and_the_undated_keep_the_order_they_were_read() {
        let mut rows = a_shelf();
        put_in_order(&mut rows, Order::Newest);
        assert_eq!(
            names(&rows),
            ["Add", "Alma", "Dora", "Bruno", "Clara"],
            "a listing sorted by release date came out in the wrong order"
        );
        assert_eq!(
            rows[0].name, "Add",
            "a head row acts on the whole shelf and was sorted into it"
        );
    }

    #[test]
    fn what_the_remote_vouches_for_comes_first_and_keeps_the_order_beneath() {
        let mut rows = a_shelf();
        put_in_order(&mut rows, Order::Verified);
        // Bruno and Clara are the vouched-for two, and they arrive in the
        // order the shelf built them rather than in one the sort invented.
        assert_eq!(names(&rows), ["Add", "Bruno", "Clara", "Alma", "Dora"]);
    }

    #[test]
    fn a_listing_that_is_not_one_run_of_applications_is_not_sorted_at_all() {
        // Home's shape: a hero, a line of type, and applications under it.
        // Sorting that would carry the heading away from what it names.
        let mut rows = some_rows();
        let before = names(&rows).join(",");
        put_in_order(&mut rows, Order::Newest);
        assert_eq!(names(&rows).join(","), before);
    }

    #[test]
    fn the_best_rated_come_first_and_the_unrated_keep_the_order_they_were_read() {
        let mut rows = vec![
            head_row(Kind::Add, "Add", "a head row", "setting-connect"),
            rated("Alma", 0.0, 0),
            rated("Bruno", 4.6, 400),
            rated("Clara", 0.0, 0),
            rated("Dora", 3.1, 12),
        ];
        put_in_order(&mut rows, Order::Rating);
        assert_eq!(names(&rows), ["Add", "Bruno", "Dora", "Alma", "Clara"]);
    }

    #[test]
    fn the_most_reviewed_come_first() {
        let mut rows = vec![
            rated("Alma", 4.0, 3),
            rated("Bruno", 4.0, 900),
            rated("Clara", 4.0, 40),
        ];
        put_in_order(&mut rows, Order::Reviews);
        assert_eq!(names(&rows), ["Bruno", "Clara", "Alma"]);
    }

    #[test]
    fn the_largest_comes_first() {
        let mut rows = vec![sized("Alma", 3), sized("Bruno", 900), sized("Clara", 40)];
        put_in_order(&mut rows, Order::Size);
        assert_eq!(names(&rows), ["Bruno", "Clara", "Alma"]);
    }

    #[test]
    fn a_shelf_is_offered_only_the_orders_it_can_really_answer() {
        let games = Shelf::Section(Section::Games);
        assert!(
            !Order::Size.can_answer(games, true),
            "a category was offered an order by a size nothing on it has: the              catalogue declares none, so nothing but an installed shelf knows"
        );
        assert!(Order::Size.can_answer(Shelf::Installed, true));
        assert!(Order::Size.can_answer(Shelf::Updates, true));

        for order in [Order::Rating, Order::Reviews] {
            assert!(order.can_answer(games, true));
            assert!(
                !order.can_answer(games, false),
                "an order that reads what people said was offered before                  anything had been heard back"
            );
        }
        for order in [Order::Best, Order::Newest, Order::Verified] {
            assert!(order.can_answer(games, false), "{}", order.title(games));
        }
    }

    #[test]
    fn a_category_opens_best_rated_and_the_other_shelves_open_as_they_did() {
        for shelf in shelves() {
            let opens_in = shelf.starting_order();
            if let Shelf::Section(section) = shelf {
                assert_eq!(
                    opens_in,
                    Order::Rating,
                    "the {} category opened in {}, and the alphabet says nothing \
                     about which of a wall of applications is worth a press",
                    section.title(),
                    opens_in.shown(shelf)
                );
            } else {
                assert_eq!(
                    opens_in,
                    Order::Best,
                    "{} opened in an order nobody asked it for",
                    shelf.title()
                );
            }
        }
    }

    #[test]
    fn a_category_is_by_name_until_anything_has_been_heard_back() {
        let games = Shelf::Section(Section::Games);
        assert_eq!(
            Order::in_force(None, games, false),
            Order::Best,
            "a category sorted by a rating before ODRS had said a word"
        );
        assert_eq!(Order::in_force(None, games, true), Order::Rating);
        // And the fall back is a fall back, not a shelf that never comes back:
        // the answer arriving marks the shelf stale, which builds and sorts it
        // again. See `Store::advance`.
    }

    #[test]
    fn what_was_asked_for_outranks_the_order_a_shelf_opens_in() {
        let games = Shelf::Section(Section::Games);
        assert_eq!(
            Order::in_force(Some(Order::Best), games, true),
            Order::Best,
            "a category that was asked for the alphabet gave a rating instead"
        );
        assert_eq!(
            Order::in_force(Some(Order::Rating), Shelf::Search, true),
            Order::Rating,
            "a search that was asked for a rating kept its own ranking"
        );
        // Kept across shelves, and still only where it means something: Size
        // is asked for on Installed and there is none on a category.
        assert_eq!(
            Order::in_force(Some(Order::Size), games, true),
            Order::Best,
            "a category answered an order by a size nothing on it has"
        );
    }

    #[test]
    fn the_light_is_placed_when_it_crosses_into_the_field_and_out_of_it() {
        let mut store = Store::quiet();
        let search = shelves()
            .iter()
            .position(|shelf| *shelf == Shelf::Search)
            .expect("the Search shelf");
        store.shelf = search;
        store.column = Column::Shelves;

        // Carried between two shelves, which are cut by the same rectangle.
        store.anim.light.glide([0.0, 0.0, 100.0, 40.0], 1.0 / 60.0);
        store.stand_in(Column::Shelves);
        let carried = store
            .anim
            .light
            .glide([500.0, 0.0, 100.0, 40.0], 1.0 / 60.0);
        assert!(
            carried[0] > 0.0 && carried[0] < 500.0,
            "a light between two shelves jumped instead of crossing: {carried:?}"
        );

        // Placed on the way into the field, and again on the way out: the
        // field is a bar the width of the listing and a shelf is a row inside
        // a panel, and a light springing between them is a capsule of the
        // wrong width crossing an edge it is cut at.
        store.stand_in(Column::Field);
        let into = store
            .anim
            .light
            .glide([700.0, 10.0, 900.0, 60.0], 1.0 / 60.0);
        assert_eq!(
            into,
            [700.0, 10.0, 900.0, 60.0],
            "a light was carried out of a shelf and into the field"
        );

        store.stand_in(Column::Shelves);
        let out = store
            .anim
            .light
            .glide([20.0, 90.0, 180.0, 50.0], 1.0 / 60.0);
        assert_eq!(
            out,
            [20.0, 90.0, 180.0, 50.0],
            "a light was carried out of the field and onto a shelf, which is \
             the wide capsule that stuck out past the panel"
        );
    }

    #[test]
    fn the_pointer_reaches_the_field_only_where_there_is_one() {
        let mut store = Store::quiet();
        let search = shelves()
            .iter()
            .position(|shelf| *shelf == Shelf::Search)
            .expect("the Search shelf");

        store.shelf = search;
        store.column = Column::Shelves;
        store.point_at_field();
        assert_eq!(
            store.column,
            Column::Field,
            "a click on the search field did not reach it"
        );

        // Every other shelf draws no field, so nothing may put the light in
        // one: a column with nothing drawn in it takes presses nothing answers.
        store.shelf = shelves()
            .iter()
            .position(|shelf| *shelf == Shelf::Installed)
            .expect("the Installed shelf");
        store.column = Column::Shelves;
        store.point_at_field();
        assert_eq!(
            store.column,
            Column::Shelves,
            "the light was put in a field that is not on this shelf"
        );
    }

    #[test]
    fn only_a_shelf_that_is_a_list_of_applications_can_be_reordered() {
        for shelf in [
            Shelf::Search,
            Shelf::Updates,
            Shelf::Installed,
            Shelf::Section(Section::Games),
        ] {
            assert!(shelf.takes_order(), "{:?}", shelf.title());
        }
        for shelf in [Shelf::Home, Shelf::Repositories] {
            assert!(!shelf.takes_order(), "{:?}", shelf.title());
        }
    }

    #[test]
    fn every_order_is_named_on_every_shelf_that_offers_one() {
        for shelf in shelves() {
            if !shelf.takes_order() {
                continue;
            }
            for order in ORDERS {
                assert!(!order.title(shelf).is_empty());
                assert!(!order.shown(shelf).is_empty());
            }
        }
        // The one label that is not the same everywhere: what "best" means on
        // Search is a ranking, and everywhere else it is the alphabet.
        assert_eq!(Order::Best.title(Shelf::Search), "Best match");
        assert_eq!(
            Order::Best.title(Shelf::Section(Section::Games)),
            "Name (A to Z)"
        );
    }

    /// A listing of a hero, a heading, eight applications and a head row, which
    /// is every shape a line can be.
    fn some_rows() -> Vec<Row> {
        let mut rows = vec![Row {
            id: "org.example.Featured".into(),
            name: "Featured".into(),
            summary: "the promoted application".into(),
            developer: "Example".into(),
            icon: None,
            icon_url: None,
            screenshot: None,
            kind: Kind::Featured,
            installed: false,
            updatable: false,
            verified: false,
            released: 0,
            rating: 0.0,
            reviews: 0,
            size: 0,
            glyph: "launch",
        }];
        rows.push(Row::heading("Popular", "what everybody wants"));
        for at in 0..8 {
            rows.push(Row {
                id: format!("org.example.A{at}"),
                name: format!("A{at}"),
                summary: String::new(),
                developer: String::new(),
                icon: None,
                icon_url: None,
                screenshot: None,
                kind: Kind::App,
                installed: false,
                updatable: false,
                verified: false,
                released: 0,
                rating: 0.0,
                reviews: 0,
                size: 0,
                glyph: "launch",
            });
        }
        rows.push(head_row(Kind::Add, "Add", "a head row", "setting-connect"));
        rows
    }

    #[test]
    fn a_head_row_and_a_heading_each_take_a_line_and_the_rest_flow() {
        let lines = lay_out(&some_rows(), 3);
        let shape: Vec<(LineKind, usize)> = lines
            .iter()
            .map(|line| (line.kind, line.rows.len()))
            .collect();
        assert_eq!(
            shape,
            [
                (LineKind::Hero, 1),
                (LineKind::Heading, 1),
                (LineKind::Cells, 3),
                (LineKind::Cells, 3),
                (LineKind::Cells, 2),
                (LineKind::Wide, 1),
            ],
            "the grid did not come out the shape the page draws"
        );
    }

    #[test]
    fn a_narrower_window_lays_the_same_rows_out_deeper() {
        let rows = some_rows();
        let wide = lay_out(&rows, 3);
        let narrow = lay_out(&rows, 1);
        assert!(
            narrow.len() > wide.len(),
            "a narrow window drew as few lines as a wide one"
        );
        assert!(
            narrow.iter().all(|line| line.rows.len() == 1),
            "a one-wide grid put two things on a line"
        );
        assert_eq!(
            lay_out(&rows, 0).len(),
            narrow.len(),
            "a window with room for nothing was not treated as room for one"
        );
    }

    #[test]
    fn every_row_appears_on_exactly_one_line() {
        let rows = some_rows();
        for columns in 1..=4 {
            let mut seen: Vec<usize> = lay_out(&rows, columns)
                .iter()
                .flat_map(|line| line.rows.clone())
                .collect();
            seen.sort_unstable();
            assert_eq!(
                seen,
                (0..rows.len()).collect::<Vec<_>>(),
                "at {columns} across, a row was lost or drawn twice"
            );
        }
    }

    #[test]
    fn a_line_of_type_is_stepped_over_rather_than_landed_on() {
        let lines = lay_out(&some_rows(), 3);
        // Line 0 is the head row, line 1 is the heading, line 2 is the first
        // line of cards. Going down from the hero must land on the cards.
        assert_eq!(
            line_towards_in(&lines, 0, 1),
            Some(2),
            "the light came to rest on a line of type"
        );
        assert_eq!(
            line_towards_in(&lines, 2, -1),
            Some(0),
            "going back up came to rest on a line of type"
        );
        assert_eq!(
            line_towards_in(&lines, 5, 1),
            None,
            "there was something below the head row at the end"
        );
        assert_eq!(line_towards_in(&lines, 0, -1), None, "above the first");
    }

    #[test]
    fn a_row_is_found_on_the_line_it_was_laid_out_on() {
        let lines = lay_out(&some_rows(), 3);
        assert_eq!(place_in(&lines, 0), (0, 0), "the hero");
        assert_eq!(place_in(&lines, 2), (2, 0), "the first card");
        assert_eq!(place_in(&lines, 4), (2, 2), "the third card");
        assert_eq!(place_in(&lines, 5), (3, 0), "the fourth, on the next line");
        assert_eq!(
            place_in(&lines, 99),
            (0, 0),
            "a row that is not there answered with something other than the start"
        );
    }

    #[test]
    fn every_shelf_names_a_mark_the_toolkit_has() {
        for shelf in shelves() {
            assert!(
                lxb_app::lxb_toolkit::assets::glyph(shelf.glyph()).is_some(),
                "{} asks for a mark that is not in the toolkit: {}",
                shelf.title(),
                shelf.glyph()
            );
        }
    }

    /// Every mark this store asks for by name, from anywhere in it.
    ///
    /// A mark that is not there is drawn as nothing at all — no warning, no
    /// gap in the layout, just a row with a hole where its mark should be. The
    /// only way to catch that is to name them all in one place and ask.
    const MARKS: &[&str] = &[
        "uninstall",
        "refresh",
        "setting-connect",
        "file-drive",
        "do-not-disturb",
        "chosen",
        "launch",
        "search",
        "setting-info",
        "setting-typed",
        "add",
        "open-with",
        "category-internet",
        "category-development",
        "arrow-up",
        "arrow-down",
    ];

    #[test]
    fn every_mark_this_store_asks_for_is_a_mark_the_toolkit_has() {
        for glyph in MARKS {
            assert!(
                lxb_app::lxb_toolkit::assets::glyph(glyph).is_some(),
                "a row asks for a mark that is not in the toolkit: {glyph}"
            );
        }
    }

    #[test]
    fn a_listing_comes_back_up_when_the_window_it_is_in_grows() {
        let mut store = Store::quiet();
        store.rows = some_rows();
        store.columns = 1;
        store.lines = lay_out(&store.rows, 1);
        let deep = store.lines.len() as f32 * 100.0;
        let tops: Vec<f32> = (0..store.lines.len())
            .map(|line| line as f32 * 100.0)
            .collect();
        let shape = |viewport: f32| Shape {
            columns: 1,
            room: (viewport / 100.0) as usize,
            tops: tops.clone(),
            peek: 0.0,
            viewport,
            deep,
        };

        // Moving can happen before a frame has been laid out — a picture asks
        // for a row before there is one. With no shape written down there is
        // nothing to settle against, and a store that guessed at one would
        // leave the listing scrolled by a line every frame after it had room
        // for.
        store.row = 2;
        store.settle_the_window();
        assert_eq!(store.top, 0, "a listing scrolled before it was laid out");

        // At the end of the list in a window with room for three lines.
        store.row = store.rows.len() - 1;
        store.shape_is(shape(300.0));
        let end = store.lines.len() - 3;
        assert_eq!(store.top, end, "the end of the list was not on the screen");

        // The window grew: what was above it comes back rather than leaving
        // the last line hanging at the top of a page of air.
        store.shape_is(shape(600.0));
        assert_eq!(
            store.top,
            store.lines.len() - 6,
            "a listing stayed scrolled past lines the window had room for"
        );
    }

    #[test]
    fn the_panel_carries_the_chosen_shelf_with_it() {
        let mut store = Store::quiet();
        store.shelves_are(6, 60.0);
        assert_eq!(store.shelf_top, 0, "a panel scrolled with nothing off it");

        // Down to the last shelf: the panel has followed it, and the shelf is
        // inside the six rows the panel said it had room for.
        store.shelf = shelves().len() - 1;
        store.settle_the_shelves();
        assert_eq!(store.shelf_top, shelves().len() - 6);
        assert!(
            store.shelf >= store.shelf_top && store.shelf < store.shelf_top + 6,
            "the chosen shelf was scrolled off the panel it is drawn in"
        );

        // And back to the top.
        store.shelf = 0;
        store.settle_the_shelves();
        assert_eq!(store.shelf_top, 0);

        // A panel tall enough for every shelf never scrolls at all.
        store.shelf = 0;
        store.settle_the_shelves();
        store.shelves_are(shelves().len() + 4, 60.0);
        store.shelf = shelves().len() - 1;
        store.settle_the_shelves();
        assert_eq!(
            store.shelf_top, 0,
            "a panel with room for every shelf scrolled anyway"
        );
    }

    #[test]
    fn a_repository_is_named_after_the_file_that_describes_it() {
        assert_eq!(
            Adding::name_from("https://dl.flathub.org/repo/flathub.flatpakrepo"),
            "flathub"
        );
        assert_eq!(
            Adding::name_from("https://nightly.gnome.org/gnome-nightly.flatpakrepo"),
            "gnome-nightly"
        );
        assert_eq!(
            Adding::name_from(""),
            "",
            "an address of nothing was given a name anyway"
        );
    }

    #[test]
    fn a_name_somebody_typed_is_not_taken_away_by_the_address() {
        let mut adding = Adding::default();
        adding.typed_url("https://example.invalid/one.flatpakrepo".into());
        assert_eq!(adding.name, "one", "a name was not taken from the address");

        adding.named = true;
        adding.name = "mine".into();
        adding.typed_url("https://example.invalid/two.flatpakrepo".into());
        assert_eq!(
            adding.name, "mine",
            "a name somebody chose was overwritten by the address"
        );
    }

    #[test]
    fn a_repository_is_only_added_when_there_is_enough_to_add() {
        let mut adding = Adding::default();
        assert!(!adding.ready(), "nothing at all was ready to be added");
        adding.typed_url("http://example.invalid/one.flatpakrepo".into());
        assert!(
            !adding.ready(),
            "a repository would have been fetched in the clear"
        );
        adding.typed_url("https://example.invalid/one.flatpakrepo".into());
        assert!(adding.ready(), "a good address was refused: {adding:?}");
    }

    #[test]
    fn a_head_row_acts_and_the_rest_open() {
        assert!(Kind::Add.is_head());
        assert!(Kind::UpdateAll { scope: Scope::User }.is_head());
        assert!(Kind::Trim.is_head());
        assert!(!Kind::App.is_head());
        assert!(!Kind::Featured.is_head());
        assert!(!Kind::Repo {
            scope: Scope::User,
            disabled: false
        }
        .is_head());

        assert!(Kind::App.is_app());
        assert!(Kind::Featured.is_app());
        assert!(!Kind::Add.is_app());
        assert!(Kind::Featured.is_wide());
        assert!(Kind::Featured.can_be_chosen());
        assert!(Kind::Featured.opens_on_click());
        assert!(Kind::App.opens_on_click());
        assert!(Kind::Add.opens_on_click());
        assert!(Kind::Repo {
            scope: Scope::User,
            disabled: false,
        }
        .opens_on_click());
        assert!(!Kind::UpdateAll { scope: Scope::User }.opens_on_click());
        assert!(!Kind::Trim.opens_on_click());
        assert!(!Kind::Heading.opens_on_click());
        assert!(!Kind::Heading.can_be_chosen());
    }

    fn running(job: Job) -> Running {
        Running {
            name: String::new(),
            step: job.doing(),
            job,
            through: 0.0,
            at: 0,
            of: 1,
            transferred: 0,
            stopping: false,
        }
    }

    #[test]
    fn only_the_head_row_that_started_the_current_job_offers_to_stop_it() {
        let mut store = Store::quiet();
        let user_updates = Kind::UpdateAll { scope: Scope::User };
        let system_updates = Kind::UpdateAll {
            scope: Scope::System,
        };
        assert!(!store.head_is_running(&user_updates));
        assert!(!store.head_is_running(&system_updates));
        assert!(!store.head_is_running(&Kind::Trim));

        store.running = Some(running(Job::UpdateAll { scope: Scope::User }));
        assert!(store.head_is_running(&user_updates));
        assert!(!store.head_is_running(&system_updates));
        assert!(!store.head_is_running(&Kind::Trim));

        store.running = Some(running(Job::Trim { scope: Scope::User }));
        assert!(!store.head_is_running(&user_updates));
        assert!(!store.head_is_running(&system_updates));
        assert!(store.head_is_running(&Kind::Trim));

        store.running = Some(running(Job::Install {
            scope: Scope::User,
            remote: "flathub".into(),
            reference: "app/org.example.Player/x86_64/stable".into(),
        }));
        assert!(!store.head_is_running(&user_updates));
        assert!(!store.head_is_running(&system_updates));
        assert!(!store.head_is_running(&Kind::Trim));
    }

    #[test]
    fn a_busy_detail_page_only_offers_stop_for_the_job_it_owns() {
        let mut store = Store::quiet();
        store.running = Some(running(Job::Install {
            scope: Scope::User,
            remote: "flathub".into(),
            reference: "app/org.example.Player.Beta/x86_64/stable".into(),
        }));

        assert_eq!(
            store.detail_buttons("org.example.Player.Beta"),
            [Button::Stop]
        );
        assert!(
            store.detail_buttons("org.example.Player").is_empty(),
            "an application whose ID is a prefix of the running one offered Stop"
        );
    }

    #[test]
    fn a_busy_repository_page_only_offers_stop_for_the_job_it_owns() {
        let mut store = Store::quiet();
        store.running = Some(running(Job::Repository {
            scope: Scope::System,
            job: RepoJob::Refresh {
                name: "flathub-beta".into(),
            },
        }));

        assert_eq!(
            store.repository_buttons("flathub-beta", Scope::System),
            [Button::Stop]
        );
        assert!(
            store
                .repository_buttons("flathub", Scope::System)
                .is_empty(),
            "a repository whose name is a prefix of the running one offered Stop"
        );
        assert!(
            store
                .repository_buttons("flathub-beta", Scope::User)
                .is_empty(),
            "the same repository name in a different installation offered Stop"
        );
    }

    #[test]
    fn taking_something_away_is_said_before_the_press_and_not_after() {
        assert!(Button::Remove.grave());
        assert!(Button::RepoForget.grave());
        assert!(!Button::Install.grave());
        for button in [
            Button::Install,
            Button::Update,
            Button::Remove,
            Button::Open,
            Button::Stop,
            Button::RepoOn,
            Button::RepoOff,
            Button::RepoRefresh,
            Button::RepoForget,
        ] {
            assert!(!button.label().is_empty());
        }
    }

    /// The updates shelf is two lists with a line of type between them, and
    /// only the first of them is in the order.
    #[test]
    fn a_sort_moves_the_applications_and_leaves_what_they_stand_on() {
        let mut rows = vec![
            head_row(
                Kind::UpdateAll {
                    scope: Scope::System,
                },
                "Update",
                "2",
                "refresh",
            ),
            app_row("small", 10),
            app_row("large", 900),
            Row::heading("Application support", ""),
            support_row("first runtime", 5000),
            support_row("second runtime", 1),
        ];
        put_in_order(&mut rows, Order::Size);

        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Update",
                "large",
                "small",
                "Application support",
                "first runtime",
                "second runtime"
            ],
            "the runtimes were sorted into the applications, or nothing was sorted"
        );
    }

    fn app_row(name: &str, size: u64) -> Row {
        let mut row = Row::heading(name, "");
        row.kind = Kind::App;
        row.size = size;
        row
    }

    fn support_row(name: &str, size: u64) -> Row {
        let mut row = Row::heading(name, "");
        row.kind = Kind::Support {
            scope: Scope::System,
        };
        row.size = size;
        row
    }

    /// What a hand on a bar means, which is the one piece of arithmetic
    /// between a pointer and a list.
    #[test]
    fn a_bar_is_dragged_by_its_middle_and_never_off_its_own_track() {
        use super::share_along;
        // A track four hundred long with a thumb of a hundred: three hundred
        // for the thumb to move in, and its middle is what follows the hand.
        let track = [0.0, 100.0, 8.0, 400.0];

        // Taken hold of at the very top of the track: the thumb's middle
        // cannot go above its own half-length, so this is nought.
        assert_eq!(share_along(track, 100.0, 100.0), 0.0);
        // Its middle at the middle of the track is halfway down the list.
        assert_eq!(share_along(track, 100.0, 300.0), 0.5);
        // And at the foot of it, all the way down.
        assert_eq!(share_along(track, 100.0, 450.0), 1.0);
        // Dragged off either end, it stays on the track.
        assert_eq!(share_along(track, 100.0, -900.0), 0.0);
        assert_eq!(share_along(track, 100.0, 9000.0), 1.0);

        // A thumb as long as its track has nowhere to go, and must not answer
        // with a division by nothing.
        assert_eq!(share_along(track, 400.0, 250.0), 0.0);
    }

    /// Down through a row drawn on one line would walk the light sideways.
    #[test]
    fn the_light_comes_back_up_to_the_control_it_left() {
        // Merged onto one line: Down from Remove, and Up again, is Remove.
        // Landing on the tabs would move the light sideways onto About, which
        // is a different control that happens to share the line.
        assert_eq!(
            back_up(true, Band::Buttons, 2),
            Band::Buttons,
            "the light came back from the reading onto a tab it never left"
        );
        assert_eq!(
            back_up(true, Band::Tabs, 2),
            Band::Tabs,
            "the light went down from a tab and came back onto a control"
        );

        // Wrapped onto two lines the tabs are the row directly above, so the
        // light walks back up through them however far down it started.
        assert_eq!(
            back_up(false, Band::Buttons, 2),
            Band::Tabs,
            "the light skipped the row between the reading and the controls"
        );

        // A page with no controls at all — a repository that offers none —
        // has nowhere else for the light to go.
        assert_eq!(
            back_up(true, Band::Buttons, 0),
            Band::Tabs,
            "the light was sent to a row of controls that is not there"
        );
    }

    #[test]
    fn one_line_of_controls_and_tabs_is_walked_as_one_line() {
        use super::{crossing, Crossing};
        // Down out of the controls: the tabs where they are the line below,
        // and the reading itself where they stand beside them.
        assert_eq!(
            crossing(Band::Buttons, Action::Down, false, 0, 1, 0),
            Some(Crossing::To(Band::Tabs))
        );
        assert_eq!(
            crossing(Band::Buttons, Action::Down, true, 0, 1, 0),
            Some(Crossing::To(Band::Content))
        );
        // Right off the end of the controls, and only off the end of them.
        assert_eq!(
            crossing(Band::Buttons, Action::Right, true, 2, 3, 0),
            Some(Crossing::To(Band::Tabs))
        );
        assert_eq!(crossing(Band::Buttons, Action::Right, true, 1, 3, 0), None);
        assert_eq!(crossing(Band::Buttons, Action::Right, false, 2, 3, 0), None);
        // Left off the front of the tabs comes back to the last control, and
        // steps between tabs anywhere else.
        assert_eq!(
            crossing(Band::Tabs, Action::Left, true, 0, 3, 0),
            Some(Crossing::ToLastControl)
        );
        assert_eq!(crossing(Band::Tabs, Action::Left, true, 0, 3, 1), None);
        assert_eq!(crossing(Band::Tabs, Action::Left, false, 0, 3, 0), None);
        // Up out of the tabs is the controls above them, and nothing at all
        // where the controls are beside them.
        assert_eq!(
            crossing(Band::Tabs, Action::Up, false, 0, 3, 0),
            Some(Crossing::To(Band::Buttons))
        );
        assert_eq!(crossing(Band::Tabs, Action::Up, true, 0, 3, 0), None);
        // A page whose controls are all gone has nothing to cross back to.
        assert_eq!(crossing(Band::Tabs, Action::Left, true, 0, 0, 0), None);
        assert_eq!(
            crossing(Band::Buttons, Action::Down, true, 0, 0, 0),
            Some(Crossing::To(Band::Content))
        );
    }

    /// The way out is the legend's, and no page may put it back into its own
    /// row of controls: two Backs on one screen is the fault this was.
    #[test]
    fn the_way_out_is_never_one_of_a_pages_own_controls() {
        let mut store = Store::quiet();
        store.machine.remotes.push(crate::flatpak::Remote {
            name: "flathub".into(),
            title: "Flathub".into(),
            url: "https://dl.flathub.org/repo/".into(),
            scope: Scope::User,
            appstream: std::path::PathBuf::new(),
            description: String::new(),
            homepage: String::new(),
            disabled: false,
            noenumerate: false,
            gpg_verify: true,
            priority: 1,
        });
        for buttons in [
            store.detail_buttons("org.example.Player"),
            store.repository_buttons("flathub", Scope::User),
        ] {
            assert!(
                !buttons.iter().any(|button| button.label() == "Back"),
                "a row of controls carries the way out as well: {buttons:?}"
            );
        }
    }
    /// Wind the clock on until nothing is crossing any more.
    fn wind(store: &mut Store) {
        let mut at = 0.0;
        for _ in 0..60 {
            at += 1.0 / 60.0;
            store.animate(at);
        }
    }

    #[test]
    fn a_page_grows_out_of_the_card_that_was_pressed_and_shrinks_back_into_it() {
        let mut store = Store::quiet();
        store.rows = some_rows();
        store.row = 3;
        let id = store.rows[3].id.clone();
        let card = [400.0, 300.0, 380.0, 96.0];
        store.cards_are(vec![(2, [400.0, 180.0, 380.0, 96.0]), (3, card)]);

        store.screen = Screen::Detail { id: id.clone() };
        store.out_of_the_card(&id);
        assert_eq!(
            store.opened_from(),
            Some(card),
            "the page grew out of something other than the card that was pressed"
        );
        assert!(store.in_the_crossing(), "the page arrived on the press");

        // The listing is drawn again under the growing page, and its cards
        // have moved: a window resized while a page is open.
        let moved = [412.0, 260.0, 380.0, 96.0];
        store.cards_are(vec![(3, moved)]);
        assert_eq!(
            store.opened_from(),
            Some(moved),
            "a card that moved under an open page was not followed"
        );

        wind(&mut store);
        assert!(!store.in_the_crossing(), "the page never finished arriving");

        store.back_into_the_card();
        assert_eq!(
            store.screen,
            Screen::Browse,
            "the listing did not have the presses back at the press"
        );
        assert_eq!(
            store.over(),
            Some(Screen::Detail { id }),
            "the page vanished before its transition had ended"
        );

        wind(&mut store);
        assert_eq!(store.over(), None, "the page was never let go of");
        assert!(!store.in_the_crossing());
        assert_eq!(store.screen, Screen::Browse);
    }

    #[test]
    fn a_card_of_the_same_application_further_down_is_not_the_card_that_was_pressed() {
        // The hero at the head of a shelf and a card of the same application
        // in the listing under it name one thing between them.
        let mut store = Store::quiet();
        let mut rows = some_rows();
        rows[0].id = rows[3].id.clone();
        store.rows = rows;
        store.row = 3;
        let id = store.rows[3].id.clone();

        let hero = [60.0, 60.0, 1400.0, 300.0];
        let card = [400.0, 500.0, 380.0, 96.0];
        store.screen = Screen::Detail { id: id.clone() };
        store.cards_are(vec![(0, hero), (3, card)]);
        store.out_of_the_card(&id);
        store.cards_are(vec![(0, hero), (3, card)]);

        assert_eq!(
            store.opened_from(),
            Some(card),
            "the page grew out of the hero rather than the card that was pressed"
        );
    }

    #[test]
    fn a_head_row_is_never_taken_for_another_head_row() {
        // Every head row of a shelf names nothing at all, so what a page grew
        // out of can only be the row it was pressed on.
        let mut store = Store::quiet();
        store.rows = vec![
            head_row(Kind::UpdateAll { scope: Scope::User }, "Update", "", "sync"),
            head_row(Kind::Add, "Add", "", "add"),
        ];
        store.row = 1;
        let update = [60.0, 60.0, 380.0, 96.0];
        let add = [60.0, 180.0, 380.0, 96.0];

        store.screen = Screen::AddRepository;
        store.out_of_the_card("");
        store.cards_are(vec![(0, update), (1, add)]);

        assert_eq!(
            store.opened_from(),
            Some(add),
            "the page a head row opened grew out of a different head row"
        );
    }
}
