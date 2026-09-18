//! What a remote has to offer, read from the AppStream catalogue flatpak
//! already keeps on disk.
//!
//! Every remote flatpak knows about carries a `appstream.xml` and a directory
//! of cached icons beside it, refreshed by `flatpak update --appstream` and by
//! every ordinary update. That file is the catalogue: reading it is how this
//! store learns that an application exists at all, and it means a machine that
//! has been updated once can be browsed with no network at all.
//!
//! It is a large file — Flathub's is 47 MB — so it is read as a stream and
//! only the fields a store shows are kept. Everything else, and there is a
//! great deal of it, is stepped over without being turned into a string.

use std::collections::HashMap;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use quick_xml::events::Event;
use quick_xml::Reader;

/// One screenshot, at the size it will be drawn and with the shape it has.
///
/// The shape is the point. AppStream declares every image's width and height,
/// so a page can lay out a frame of exactly the right proportion **before**
/// the picture has been fetched — the frame never changes shape underneath a
/// picture arriving in it, and a wide screenshot is never shown in a tall box
/// with empty air down both sides.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shot {
    pub url: String,
    pub width: u32,
    pub height: u32,
    pub caption: String,
}

impl Shot {
    /// Width over height, where both are known.
    pub fn aspect(&self) -> Option<f32> {
        (self.width > 0 && self.height > 0).then(|| self.width as f32 / self.height as f32)
    }
}

/// One release, as the project described it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    /// The day it was published, written out. Empty where none was declared.
    pub when: String,
    /// What changed, flattened to prose the way a description is.
    pub notes: String,
}

/// Somewhere else to read about an application.
///
/// Only the kinds worth putting in front of somebody: a store is not a
/// directory of every address a project has ever published.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Homepage,
    Help,
    Bugtracker,
    Donation,
    Contact,
    Translate,
    Source,
}

impl Link {
    /// In the order they are offered, which is the order somebody wants them.
    pub const ALL: [Link; 7] = [
        Link::Homepage,
        Link::Help,
        Link::Donation,
        Link::Bugtracker,
        Link::Translate,
        Link::Contact,
        Link::Source,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Link::Homepage => crate::i18n::text("website"),
            Link::Help => crate::i18n::text("help"),
            Link::Bugtracker => crate::i18n::text("report-a-problem"),
            Link::Donation => crate::i18n::text("donate"),
            Link::Contact => crate::i18n::text("contact"),
            Link::Translate => crate::i18n::text("translate-it"),
            Link::Source => crate::i18n::text("source-code"),
        }
    }

    /// The mark beside it.
    ///
    /// Two of these are what the address really is — a website and a source
    /// repository — and the rest are all the same thing: somewhere else, which
    /// is exactly what `open-with` means. The toolkit has no mark for a bug
    /// tracker or a donation page, and inventing one out of a mark that means
    /// something else would say something untrue in every other place that
    /// mark appears.
    pub fn glyph(self) -> &'static str {
        match self {
            Link::Homepage => "category-internet",
            Link::Help => "setting-info",
            Link::Source => "category-development",
            Link::Bugtracker | Link::Donation | Link::Contact | Link::Translate => "open-with",
        }
    }

    fn of(kind: &str) -> Option<Self> {
        Some(match kind {
            "homepage" => Link::Homepage,
            "help" => Link::Help,
            "bugtracker" => Link::Bugtracker,
            "donation" => Link::Donation,
            "contact" => Link::Contact,
            "translate" => Link::Translate,
            "vcs-browser" => Link::Source,
            _ => return None,
        })
    }
}

/// One application, as a remote describes it.
#[derive(Debug, Clone, Default)]
pub struct Listing {
    /// The AppStream component id, which is also the flatpak application id.
    pub id: String,
    pub name: String,
    pub summary: String,
    /// The long description, flattened to paragraphs separated by a blank
    /// line. AppStream writes it as `<p>` and `<ul><li>`; nothing here keeps
    /// the markup, because the page draws prose rather than a document.
    pub description: String,
    pub developer: String,
    pub license: String,
    pub categories: Vec<String>,
    pub keywords: Vec<String>,
    /// The newest version a release is declared for, which is not necessarily
    /// what is installed.
    pub version: String,
    /// The cached icon on disk, if the remote shipped one at a size worth
    /// drawing. Absent for the many components that ship none.
    pub icon: Option<PathBuf>,
    /// The icon the remote will serve over the network, for the components
    /// whose cached one was never written to this disk. Fetched the way a
    /// screenshot is, and only once a page is looking at it.
    pub icon_remote: Option<String>,
    /// Screenshots, the widest usable thumbnail of each. These are the only
    /// part of a listing that is not already on this disk.
    pub screenshots: Vec<Shot>,
    /// What changed, newest first. Trimmed, because a project with two hundred
    /// releases is not two hundred rows anybody wants.
    pub releases: Vec<Release>,
    /// Where else to read about it.
    pub links: Vec<(Link, String)>,
    /// `org.gnome.Platform/x86_64/49` — what it runs on. That is the single
    /// largest thing an install fetches, so it is worth naming before a press.
    pub runtime: String,
    /// Whether the remote says the project itself publishes this. Flathub
    /// writes it into `<custom>`; a remote that says nothing is not verified,
    /// which is not the same as being disowned.
    pub verified: bool,
    /// Whether a content rating was declared at all. One declared with nothing
    /// objectionable in it is still a rating, and saying so is the point.
    pub rated: bool,
    /// `app/org.videolan.VLC/x86_64/stable` — what a transaction is given.
    pub reference: String,
    /// The remote this listing came from, by name.
    pub remote: String,
    /// Which installation that remote belongs to.
    pub scope: crate::flatpak::Scope,
    /// When the newest release was published, as the seconds AppStream writes
    /// rather than the day `Release::when` spells out. Nought where no release
    /// carried a timestamp at all, which is most of a catalogue's older half —
    /// see `Order::Newest` for what that means for a sort.
    pub released: i64,
    /// A lowercase haystack of name, summary, id and keywords, built once so
    /// that a search does not lowercase the whole catalogue on every keystroke.
    haystack: String,
}

impl Listing {
    /// The part of the id after the last dot, which is what a human would call
    /// the application when the name is missing.
    pub fn short_id(&self) -> &str {
        self.id.rsplit('.').next().unwrap_or(&self.id)
    }

    /// Whether every word of the query appears somewhere in this listing.
    ///
    /// Words rather than the whole string, so that "video player" finds an
    /// application whose name is one and whose summary is the other.
    pub fn matches(&self, words: &[String]) -> bool {
        words.iter().all(|word| self.haystack.contains(word))
    }

    /// The address of a link of one kind, if the project published one.
    pub fn link(&self, kind: Link) -> Option<&str> {
        self.links
            .iter()
            .find(|(one, _)| *one == kind)
            .map(|(_, url)| url.as_str())
    }

    /// How well a listing answers a query, lowest first.
    ///
    /// A name that starts with what was typed beats a name that merely
    /// contains it — otherwise searching for "gimp" puts every application
    /// whose description says "like GIMP" above GIMP.
    ///
    /// The summary is ranked as well as the name, and that is what makes a
    /// query of more than one word work: nothing is called "video editor", but
    /// Kdenlive's summary is exactly that, and a store that ranked names alone
    /// would answer with whatever came first in the alphabet.
    pub fn rank(&self, query: &str) -> u8 {
        let name = self.name.to_lowercase();
        let summary = self.summary.to_lowercase();
        if name == query {
            0
        } else if name.starts_with(query) {
            1
        } else if name.contains(query) {
            2
        } else if summary == query {
            3
        } else if summary.starts_with(query) {
            4
        } else if summary.contains(query) {
            5
        } else if self.id.to_lowercase().contains(query) {
            6
        } else {
            7
        }
    }
}

/// Every listing on the machine, indexed by id.
#[derive(Debug, Default)]
pub struct Catalogue {
    pub listings: Vec<Listing>,
    by_id: HashMap<String, usize>,
}

impl Catalogue {
    /// Read every enabled remote of every installation.
    ///
    /// A remote that is disabled, or that says it should not be enumerated, is
    /// deliberately skipped: those are the single-application origins flatpak
    /// writes when something is installed from a bundle, and their catalogue
    /// is not something to browse.
    pub fn read(remotes: &[&crate::flatpak::Remote]) -> Self {
        let mut catalogue = Self::default();
        for remote in remotes {
            let file = remote.appstream.join("appstream.xml");
            let icons = remote.appstream.join("icons");
            match read_file(&file, &icons, &remote.name, remote.scope) {
                Ok(listings) => catalogue.absorb(listings),
                Err(trouble) => {
                    eprintln!("distribumpy: {}: {trouble}", file.display());
                }
            }
        }
        catalogue.listings.sort_by(|one, other| {
            one.name
                .to_lowercase()
                .cmp(&other.name.to_lowercase())
                .then_with(|| one.id.cmp(&other.id))
        });
        catalogue.by_id = catalogue
            .listings
            .iter()
            .enumerate()
            .map(|(at, listing)| (listing.id.clone(), at))
            .collect();
        catalogue
    }

    /// The first listing for an id.
    ///
    /// One application can be offered by more than one remote, and by both
    /// installations of the same remote. The first is kept, and because the
    /// user installation is read first, that is the one nothing has to
    /// authorise.
    pub fn get(&self, id: &str) -> Option<&Listing> {
        self.by_id.get(id).map(|at| &self.listings[*at])
    }

    /// How many listings came from one remote, which is the only honest
    /// thing a store can say about how much a repository offers.
    pub fn count_from(&self, remote: &str) -> usize {
        self.listings
            .iter()
            .filter(|one| one.remote == remote)
            .count()
    }

    fn absorb(&mut self, listings: Vec<Listing>) {
        let known: std::collections::HashSet<String> =
            self.listings.iter().map(|one| one.id.clone()).collect();
        self.listings
            .extend(listings.into_iter().filter(|one| !known.contains(&one.id)));
    }

    /// Everything matching a query, best answers first.
    pub fn search(&self, query: &str) -> Vec<&Listing> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        let words: Vec<String> = query.split_whitespace().map(str::to_string).collect();
        let mut found: Vec<&Listing> = self
            .listings
            .iter()
            .filter(|listing| listing.matches(&words))
            .collect();
        found.sort_by(|one, other| {
            one.rank(&query)
                .cmp(&other.rank(&query))
                .then_with(|| one.name.to_lowercase().cmp(&other.name.to_lowercase()))
        });
        found
    }

    /// Everything in one of the store's own sections.
    pub fn section(&self, section: Section) -> Vec<&Listing> {
        self.listings
            .iter()
            .filter(|listing| section.holds(listing))
            .collect()
    }
}

/// The store's own shelves.
///
/// These are the XDG main categories the shell itself sorts applications into,
/// so an application found here lands on the column it will appear in once it
/// is installed. `Everything` is not a category — it is the whole catalogue,
/// and it is what a store without one would be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Everything,
    Multimedia,
    Graphics,
    Internet,
    Office,
    Games,
    Development,
    Education,
    Utilities,
    System,
}

impl Section {
    /// In the order they are shown, which is the shell's own lattice order so
    /// that the two read the same way.
    pub const ALL: [Section; 10] = [
        Section::Everything,
        Section::Games,
        Section::Multimedia,
        Section::Graphics,
        Section::Internet,
        Section::Office,
        Section::Development,
        Section::Education,
        Section::Utilities,
        Section::System,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Section::Everything => crate::i18n::text("everything"),
            Section::Multimedia => crate::i18n::text("multimedia"),
            Section::Graphics => crate::i18n::text("graphics"),
            Section::Internet => crate::i18n::text("internet"),
            Section::Office => crate::i18n::text("office"),
            Section::Games => crate::i18n::text("games"),
            Section::Development => crate::i18n::text("development"),
            Section::Education => crate::i18n::text("education-science"),
            Section::Utilities => crate::i18n::text("utilities"),
            Section::System => crate::i18n::text("system"),
        }
    }

    /// The mark drawn beside the section, from the toolkit's own set.
    pub fn glyph(self) -> &'static str {
        match self {
            Section::Everything => "launch",
            Section::Multimedia => "category-multimedia",
            Section::Graphics => "category-graphics",
            Section::Internet => "category-internet",
            Section::Office => "category-office",
            Section::Games => "category-games",
            Section::Development => "category-development",
            Section::Education => "category-education",
            Section::Utilities => "category-utilities",
            Section::System => "category-system",
        }
    }

    /// The XDG main categories that land on this shelf. The first match wins,
    /// exactly as it does in the shell.
    fn xdg(self) -> &'static [&'static str] {
        match self {
            Section::Everything => &[],
            Section::Multimedia => &["AudioVideo", "Audio", "Video"],
            Section::Graphics => &["Graphics"],
            Section::Internet => &["Network"],
            Section::Office => &["Office"],
            Section::Games => &["Game"],
            Section::Development => &["Development"],
            Section::Education => &["Education", "Science"],
            Section::Utilities => &["Utility"],
            Section::System => &["Settings", "System"],
        }
    }

    fn holds(self, listing: &Listing) -> bool {
        if self == Section::Everything {
            return true;
        }
        listing
            .categories
            .iter()
            .any(|category| self.xdg().contains(&category.as_str()))
    }
}

fn read_file(
    path: &Path,
    icons: &Path,
    remote: &str,
    scope: crate::flatpak::Scope,
) -> Result<Vec<Listing>, String> {
    read_file_for(
        path,
        icons,
        remote,
        scope,
        lxb_app::lxb_toolkit::i18n::language(),
    )
}

/// The same, in a language named rather than the session's.
///
/// Split out for the tests: which of an application's names, summaries and
/// descriptions wins is the thing they are about, and a test that read the
/// session's language would say something different on a Polish machine.
fn read_file_for(
    path: &Path,
    icons: &Path,
    remote: &str,
    scope: crate::flatpak::Scope,
    locale: &str,
) -> Result<Vec<Listing>, String> {
    let file = std::fs::File::open(path)
        .map_err(|err| crate::message!("file-cannot-be-read", "why" => (err).to_string()))?;
    let mut reader = Reader::from_reader(BufReader::with_capacity(1 << 20, file));
    // Deliberately **not** trimmed by the reader.
    //
    // An entity splits an element's text into pieces — `world&apos;s` arrives
    // as "world", the entity, and "s" — and trimming each piece as it comes
    // eats the spaces between them, so `Steam &amp; friends` would be gathered
    // as `Steam&friends`. The pieces are gathered whole and the whole is
    // tidied once, at the end of the element.
    reader.config_mut().trim_text(false);
    parse_for(&mut reader, icons, remote, scope, locale)
}

/// Which element's text is being collected, if any.
///
/// AppStream repeats every translatable element once per language, and the
/// translations carry `xml:lang`. The selected session language wins, with the
/// untranslated English field as fallback, independent of element order.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
enum Collecting {
    Nothing,
    Id,
    Name,
    Summary,
    Description,
    ReleaseNote,
    Developer,
    License,
    Category,
    Keyword,
    Bundle,
    Icon,
    IconRemote,
    Screenshot,
    Caption,
    Url,
    Verified,
}

/// The most releases worth keeping.
///
/// A project with two hundred of them is ordinary, and none past the first
/// handful is what somebody opened a store to read.
const MOST_RELEASES: usize = 8;

/// The `<custom>` key Flathub writes when a project publishes its own
/// application, which is the only such claim this store repeats.
const VERIFIED_KEY: &str = "flathub::verification::verified";

struct Parsing {
    listing: Listing,
    collecting: Collecting,
    /// Depth inside `<description>`, so that its `<p>` and `<li>` are gathered
    /// while their tags are not.
    describing: bool,
    /// Set while inside an element carrying `xml:lang`, so its text is dropped.
    translated: bool,
    locale: String,
    languages: Vec<u8>,
    preferred: std::collections::BTreeMap<Collecting, u8>,
    /// Set while inside `<developer>`, whose `<name>` would otherwise be taken
    /// for the application's own.
    in_developer: bool,
    /// Set while inside `<provides>`, whose `<id>` is another application this
    /// one stands in for and must never replace its own.
    in_provides: bool,
    in_screenshots: bool,
    /// Set while inside `<releases>`, whose `<description>` belongs to one
    /// release rather than to the application.
    in_releases: bool,
    /// The width and height declared on the `<icon>` or `<image>` being read.
    /// A component lists its icon at three sizes and its screenshots at five;
    /// the widest of each that is still worth drawing is the one kept.
    width: u32,
    height: u32,
    icon_best: u32,
    icon_remote_best: u32,
    shot: Shot,
    /// The text of the element being read, gathered whole.
    ///
    /// Gathered rather than taken as it arrives, because an element's text
    /// does not arrive in one piece: every entity in it — and an apostrophe in
    /// a summary is an entity — breaks it into another one. Believing the
    /// first piece meant Audacity's summary was "Audacity is the world".
    text: String,
    /// The kind of `<url>` being read, if it is one worth keeping.
    url_kind: Option<Link>,
    /// The release being read, if any.
    release: Option<Release>,
}

impl Parsing {
    fn new(locale: &str) -> Self {
        Self {
            listing: Listing::default(),
            collecting: Collecting::Nothing,
            describing: false,
            translated: false,
            locale: lxb_app::lxb_toolkit::i18n::language_from(locale).to_owned(),
            languages: Vec::new(),
            preferred: Default::default(),
            in_developer: false,
            in_provides: false,
            in_screenshots: false,
            in_releases: false,
            width: 0,
            height: 0,
            icon_best: 0,
            icon_remote_best: 0,
            shot: Shot::default(),
            text: String::new(),
            url_kind: None,
            release: None,
        }
    }
}

/// The widest screenshot worth fetching.
///
/// Flathub publishes each screenshot at 224, 624, 752 and 1248 pixels wide
/// beside the original. A store draws one across part of a page, so the 752 is
/// the last size that earns its download; above it the picture is scaled back
/// down before it is ever seen.
const WIDEST_SCREENSHOT: u32 = 752;

fn parse_for(
    reader: &mut Reader<BufReader<std::fs::File>>,
    icons: &Path,
    remote: &str,
    scope: crate::flatpak::Scope,
    locale: &str,
) -> Result<Vec<Listing>, String> {
    let mut listings = Vec::new();
    let mut buffer = Vec::with_capacity(1 << 16);
    let mut state = Parsing::new(locale);
    let mut inside = false;

    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|err| crate::message!("catalogue-is-malformed", "why" => (err).to_string()))?;
        match event {
            Event::Eof => break,
            Event::Start(tag) => {
                let name = tag.name();
                let name = name.as_ref();
                if name == b"component" {
                    inside = true;
                    state = Parsing::new(locale);
                    state.listing.remote = remote.to_string();
                    state.listing.scope = scope;
                    buffer.clear();
                    continue;
                }
                if inside {
                    opened(&mut state, &tag, name);
                }
            }
            Event::Empty(tag) => {
                if inside {
                    empty(&mut state, &tag);
                }
            }
            Event::Text(text) => {
                if inside && state.collecting != Collecting::Nothing {
                    let value = text
                        .decode()
                        .map_err(|err| crate::message!("catalogue-text-not-utf8", "why" => (err).to_string()))?;
                    state.text.push_str(&value);
                }
            }
            Event::CData(text) => {
                if inside && state.collecting != Collecting::Nothing {
                    let value = text
                        .decode()
                        .map_err(|err| crate::message!("catalogue-text-not-utf8", "why" => (err).to_string()))?;
                    state.text.push_str(&value);
                }
            }
            Event::GeneralRef(entity) => {
                if inside && state.collecting != Collecting::Nothing {
                    let named = entity
                        .decode()
                        .map_err(|err| crate::message!("catalogue-text-not-utf8", "why" => (err).to_string()))?;
                    if let Some(letter) = resolve(&named) {
                        state.text.push(letter);
                    }
                }
            }
            Event::End(tag) => {
                let name = tag.name();
                let name = name.as_ref();
                if name == b"component" {
                    inside = false;
                    if let Some(listing) =
                        finish(std::mem::replace(&mut state, Parsing::new(locale)).listing)
                    {
                        listings.push(listing);
                    }
                    buffer.clear();
                    continue;
                }
                if inside {
                    let gathered = tidy(std::mem::take(&mut state.text));
                    if !gathered.is_empty() && !state.translated {
                        take(&mut state, icons, gathered);
                    }
                    closed(&mut state, name);
                }
            }
            _ => {}
        }
        buffer.clear();
    }

    Ok(listings)
}

/// An opening tag inside a component: what it turns collection on for, and
/// which region of the component it puts the parser in.
fn opened(state: &mut Parsing, tag: &quick_xml::events::BytesStart, name: &[u8]) {
    let rank = match attribute(tag, b"xml:lang") {
        Some(locale) => {
            let base = locale.split(['_', '-', '.', '@']).next().unwrap_or("");
            if base.eq_ignore_ascii_case(&state.locale) {
                2
            } else if base == "en" {
                1
            } else {
                0
            }
        }
        None => state.languages.last().copied().unwrap_or(1),
    };
    state.languages.push(rank);
    state.translated = rank == 0;
    state.collecting = match name {
        b"id" if !state.in_provides => Collecting::Id,
        b"name" if state.in_developer => Collecting::Developer,
        b"name" => Collecting::Name,
        b"summary" => Collecting::Summary,
        b"project_license" => Collecting::License,
        b"developer_name" => Collecting::Developer,
        b"p" | b"li" if state.release.is_some() => Collecting::ReleaseNote,
        b"p" | b"li" if state.describing => Collecting::Description,
        b"category" => Collecting::Category,
        b"keyword" => Collecting::Keyword,
        b"caption" if state.in_screenshots => Collecting::Caption,
        b"bundle" if attribute(tag, b"type").as_deref() == Some("flatpak") => {
            if let Some(runtime) = attribute(tag, b"runtime") {
                state.listing.runtime = runtime;
            }
            Collecting::Bundle
        }
        b"url" => {
            state.url_kind = attribute(tag, b"type").as_deref().and_then(Link::of);
            Collecting::Url
        }
        b"value" if attribute(tag, b"key").as_deref() == Some(VERIFIED_KEY) => Collecting::Verified,
        b"icon" if attribute(tag, b"type").as_deref() == Some("cached") => {
            state.width = number(tag, b"width");
            Collecting::Icon
        }
        b"icon" if attribute(tag, b"type").as_deref() == Some("remote") => {
            state.width = number(tag, b"width");
            Collecting::IconRemote
        }
        b"image" if state.in_screenshots => {
            state.width = number(tag, b"width");
            state.height = number(tag, b"height");
            Collecting::Screenshot
        }
        b"release" if state.in_releases => {
            begin_release(state, tag);
            Collecting::Nothing
        }
        _ => Collecting::Nothing,
    };
    match name {
        b"developer" => state.in_developer = true,
        b"provides" => state.in_provides = true,
        b"description" if !state.in_releases => state.describing = !state.translated,
        b"screenshots" => state.in_screenshots = true,
        b"releases" => state.in_releases = true,
        b"screenshot" => {
            state.shot = Shot::default();
            state.preferred.remove(&Collecting::Caption);
        }
        b"content_rating" => state.listing.rated = true,
        _ => {}
    }
    state.text.clear();
}

/// One entity, as a character.
///
/// The five XML predefines and numeric references, which is every entity a
/// catalogue with no document type of its own is allowed to contain.
fn resolve(named: &str) -> Option<char> {
    match named {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let digits = named.strip_prefix('#')?;
            let number = match digits.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => digits.parse().ok()?,
            };
            char::from_u32(number)
        }
    }
}

/// Whitespace as prose wants it: one space between words, and none at either
/// end. A catalogue wraps and indents its paragraphs, and none of that layout
/// is the text.
fn tidy(gathered: String) -> String {
    if !gathered
        .chars()
        .any(|letter| letter.is_whitespace() && letter != ' ')
        && !gathered.contains("  ")
    {
        return gathered.trim().to_string();
    }
    gathered.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A tag with no content of its own. Two matter: a release declared entirely
/// in its attributes, which is the common shape, and a content rating with
/// nothing objectionable declared in it, which is a rating all the same.
fn empty(state: &mut Parsing, tag: &quick_xml::events::BytesStart) {
    match tag.name().as_ref() {
        b"release" if state.in_releases => {
            begin_release(state, tag);
            end_release(state);
        }
        b"content_rating" => state.listing.rated = true,
        _ => {}
    }
}

/// A closing tag inside a component.
fn closed(state: &mut Parsing, name: &[u8]) {
    match name {
        b"developer" => state.in_developer = false,
        b"provides" => state.in_provides = false,
        b"description" if !state.in_releases => state.describing = false,
        b"screenshots" => state.in_screenshots = false,
        b"releases" => state.in_releases = false,
        b"release" => end_release(state),
        b"screenshot" if !state.shot.url.is_empty() => {
            let shot = std::mem::take(&mut state.shot);
            state.listing.screenshots.push(shot);
        }
        _ => {}
    }
    state.collecting = Collecting::Nothing;
    state.languages.pop();
    state.translated = state.languages.last().copied() == Some(0);
}

fn begin_release(state: &mut Parsing, tag: &quick_xml::events::BytesStart) {
    state.preferred.remove(&Collecting::ReleaseNote);
    let version = attribute(tag, b"version").unwrap_or_default();
    // The newest release is the first one written, and it is what the shelves
    // call the version on offer — kept even past the point where the list of
    // releases itself stops being worth gathering.
    if state.listing.version.is_empty() {
        state.listing.version.clone_from(&version);
    }
    let stamp = attribute(tag, b"timestamp").and_then(|stamp| stamp.parse::<i64>().ok());
    // The newest of them, not the first one read: a project is free to write
    // its releases in whatever order it likes, and one that writes them oldest
    // first would otherwise be dated by the release nobody is running. Taken
    // before the cap below, so a project with more releases than are worth
    // keeping is still dated by its latest.
    state.listing.released = state.listing.released.max(stamp.unwrap_or_default());
    if state.listing.releases.len() >= MOST_RELEASES {
        state.release = None;
        return;
    }
    state.release = Some(Release {
        version,
        when: stamp
            .map(day)
            .or_else(|| attribute(tag, b"date"))
            .unwrap_or_default(),
        notes: String::new(),
    });
}

fn end_release(state: &mut Parsing) {
    let Some(release) = state.release.take() else {
        return;
    };
    if release.version.is_empty() && release.notes.is_empty() {
        return;
    }
    state.listing.releases.push(release);
}

fn take(state: &mut Parsing, icons: &Path, value: String) {
    if matches!(
        state.collecting,
        Collecting::Name
            | Collecting::Summary
            | Collecting::Description
            | Collecting::ReleaseNote
            | Collecting::Developer
            | Collecting::Keyword
            | Collecting::Caption
    ) {
        let rank = state.languages.last().copied().unwrap_or(1);
        let best = state.preferred.entry(state.collecting).or_default();
        if rank < *best {
            return;
        }
        if rank > *best {
            match state.collecting {
                Collecting::Name => state.listing.name.clear(),
                Collecting::Summary => state.listing.summary.clear(),
                Collecting::Developer => state.listing.developer.clear(),
                Collecting::Caption => state.shot.caption.clear(),
                Collecting::Description => state.listing.description.clear(),
                Collecting::ReleaseNote => {
                    if let Some(release) = &mut state.release {
                        release.notes.clear();
                    }
                }
                Collecting::Keyword => state.listing.keywords.clear(),
                _ => {}
            }
            *best = rank;
        }
    }

    match state.collecting {
        Collecting::Nothing => {}
        Collecting::Id if state.listing.id.is_empty() => state.listing.id = value,
        Collecting::Name if state.listing.name.is_empty() => state.listing.name = value,
        Collecting::Summary if state.listing.summary.is_empty() => state.listing.summary = value,
        Collecting::Developer if state.listing.developer.is_empty() => {
            state.listing.developer = value;
        }
        Collecting::License if state.listing.license.is_empty() => state.listing.license = value,
        Collecting::Id
        | Collecting::Name
        | Collecting::Summary
        | Collecting::Developer
        | Collecting::License => {}
        Collecting::Description => add_prose(&mut state.listing.description, &value),
        Collecting::ReleaseNote => {
            if let Some(release) = &mut state.release {
                add_prose(&mut release.notes, &value);
            }
        }
        Collecting::Category => state.listing.categories.push(value),
        Collecting::Keyword => state.listing.keywords.push(value),
        Collecting::Bundle => state.listing.reference = value,
        Collecting::Caption if state.shot.caption.is_empty() => {
            state.shot.caption = value.trim().to_string();
        }
        Collecting::Caption => {}
        Collecting::Url => {
            if let Some(kind) = state.url_kind.take() {
                let url = value.trim().to_string();
                if url.starts_with("http") && state.listing.link(kind).is_none() {
                    state.listing.links.push((kind, url));
                }
            }
        }
        Collecting::Verified => state.listing.verified = value.trim() == "true",
        Collecting::Icon => {
            if state.width > state.icon_best {
                state.icon_best = state.width;
                state.listing.icon = Some(
                    icons
                        .join(format!("{}x{}", state.width, state.width))
                        .join(value.trim()),
                );
            }
        }
        Collecting::IconRemote => {
            // The plain size and never the `@2` one beside it: twice the
            // download for a picture drawn at the same number of points.
            let url = value.trim();
            if state.width > state.icon_remote_best && !url.contains("@2") {
                state.icon_remote_best = state.width;
                state.listing.icon_remote = Some(url.to_string());
            }
        }
        Collecting::Screenshot => {
            if state.width <= WIDEST_SCREENSHOT && state.width > state.shot.width {
                state.shot.url = value.trim().to_string();
                state.shot.width = state.width;
                state.shot.height = state.height;
            }
        }
    }
}

/// Add one paragraph to prose being gathered, keeping the blank line between
/// paragraphs that is the only markup this store keeps.
fn add_prose(into: &mut String, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    if !into.is_empty() {
        into.push_str("\n\n");
    }
    into.push_str(value);
}

/// A component is only a listing if it is an application that can be installed
/// and shown: runtimes, extensions and locale packs all appear in the same
/// file and none of them belongs on a shelf.
fn finish(mut listing: Listing) -> Option<Listing> {
    if listing.id.is_empty() || !listing.reference.starts_with("app/") {
        return None;
    }

    // The id is taken from the flatpak reference rather than from `<id>`.
    //
    // AppStream still carries components whose id keeps the old `.desktop`
    // suffix — EasyEffects is `com.github.wwmm.easyeffects.desktop` on Flathub
    // today — while the thing flatpak installs is called
    // `com.github.wwmm.easyeffects`. Every comparison this store makes is
    // against what flatpak calls it: whether it is installed, whether it can
    // be updated, which listing an installed application belongs to. Believing
    // `<id>` meant an application that was installed was offered as though it
    // were not.
    if let Some(named) = listing.reference.split('/').nth(1) {
        if !named.is_empty() {
            listing.id = named.to_string();
        }
    }
    if listing.name.is_empty() {
        listing.name = listing.short_id().to_string();
    }
    listing.haystack = format!(
        "{} {} {} {}",
        listing.name.to_lowercase(),
        listing.summary.to_lowercase(),
        listing.id.to_lowercase(),
        listing.keywords.join(" ").to_lowercase()
    );
    if let Some(icon) = &listing.icon {
        if !icon.exists() {
            listing.icon = None;
        }
    }
    Some(listing)
}

/// A day, written out from a unix timestamp.
///
/// The calendar arithmetic is here rather than pulled in as a crate: a store
/// needs one date, in one format, with no time zone in it, and this is the
/// whole of what that takes.
fn day(stamp: i64) -> String {
    if stamp <= 0 {
        return String::new();
    }
    let mut days = stamp / 86_400;
    let mut year: i64 = 1970;
    loop {
        let length = if leap(year) { 366 } else { 365 };
        if days < length {
            break;
        }
        days -= length;
        year += 1;
    }
    let lengths = [
        31,
        if leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 0;
    while month < 11 && days >= lengths[month] {
        days -= lengths[month];
        month += 1;
    }
    crate::message!("release-date", "day" => (days + 1).to_string(), "month" => lxb_app::lxb_toolkit::i18n::month(month + 1), "year" => year.to_string())
}

fn leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn attribute(tag: &quick_xml::events::BytesStart, wanted: &[u8]) -> Option<String> {
    tag.attributes().flatten().find_map(|attribute| {
        (attribute.key.as_ref() == wanted)
            .then(|| String::from_utf8_lossy(attribute.value.as_ref()).into_owned())
    })
}

fn number(tag: &quick_xml::events::BytesStart, wanted: &[u8]) -> u32 {
    attribute(tag, wanted)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A catalogue with everything in it that has ever needed handling: a
    /// translated name beside the real one, an id inside `<provides>`, a
    /// runtime that is not an application, three icon sizes and five
    /// screenshot sizes.
    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<components version="0.8" origin="flatpak">
  <component type="desktop-application">
    <id>org.example.Player</id>
    <name>Player</name>
    <name xml:lang="pl">Odtwarzacz</name>
    <summary>Player is the world&apos;s most popular way to play things &amp; more</summary>
    <summary xml:lang="pl">Odtwarza rzeczy</summary>
    <project_license>GPL-3.0</project_license>
    <description>
      <p>
        The first paragraph, wrapped
        across three lines and carrying Ren&#233;&apos;s name.
      </p>
      <ul><li>A listed thing.</li></ul>
    </description>
    <description xml:lang="pl"><p>Pierwszy akapit.</p></description>
    <developer><name>An Author</name></developer>
    <icon height="48" type="cached" width="48">org.example.Player.png</icon>
    <icon height="128" type="cached" width="128">org.example.Player.png</icon>
    <icon type="stock">org.example.Player</icon>
    <icon height="128" type="remote" width="128">https://example.invalid/icon.png</icon>
    <icon height="128" scale="2" type="remote" width="128">https://example.invalid/icon@2.png</icon>
    <url type="homepage">https://example.invalid/</url>
    <url type="bugtracker">https://example.invalid/bugs</url>
    <url type="faq">https://example.invalid/faq</url>
    <categories>
      <category>AudioVideo</category>
      <category>Player</category>
    </categories>
    <provides>
      <id>org.example.player.old</id>
      <mediatype>audio/ogg</mediatype>
    </provides>
    <keywords><keyword>music</keyword></keywords>
    <screenshots>
      <screenshot type="default">
        <caption>Playing something</caption>
        <caption xml:lang="pl">Odtwarzanie</caption>
        <image height="720" type="source" width="1280">https://example.invalid/orig.png</image>
        <image height="702" type="thumbnail" width="1248">https://example.invalid/1248.png</image>
        <image height="423" type="thumbnail" width="752">https://example.invalid/752.png</image>
        <image height="125" type="thumbnail" width="224">https://example.invalid/224.png</image>
      </screenshot>
    </screenshots>
    <releases>
      <release timestamp="1767225600" type="stable" version="4.2.0">
        <description><p>Everything is faster.</p></description>
      </release>
      <release timestamp="1735689600" type="stable" version="4.1.0"/>
    </releases>
    <content_rating type="oars-1.1"/>
    <custom><value key="flathub::verification::verified">true</value></custom>
    <bundle type="flatpak" runtime="org.example.Platform/x86_64/25.08">app/org.example.Player/x86_64/stable</bundle>
  </component>
  <component type="runtime">
    <id>org.example.Platform</id>
    <name>A Runtime</name>
    <bundle type="flatpak">runtime/org.example.Platform/x86_64/25.08</bundle>
  </component>
  <component type="desktop-application">
    <id>org.example.Nameless</id>
    <summary>Has no name of its own</summary>
    <bundle type="flatpak">app/org.example.Nameless/x86_64/stable</bundle>
  </component>
  <component type="desktop-application">
    <id>org.example.Old.desktop</id>
    <name>Still Here</name>
    <summary>Carries the old suffixed id</summary>
    <bundle type="flatpak">app/org.example.Old/x86_64/stable</bundle>
  </component>
</components>
"#;

    #[test]
    fn polish_appstream_fields_win_in_either_order_and_keep_protocol_data() {
        for sample in [
            SAMPLE.to_owned(),
            SAMPLE.replace(
                "<name>Player</name>\n    <name xml:lang=\"pl\">Odtwarzacz</name>",
                "<name xml:lang=\"pl\">Odtwarzacz</name>\n    <name>Player</name>",
            ),
        ] {
            let holder = tempdir::Holder::new();
            let path = holder.path().join("translated.xml");
            std::fs::write(&path, sample).unwrap();
            let mut reader =
                Reader::from_reader(BufReader::new(std::fs::File::open(&path).unwrap()));
            let listings = parse_for(
                &mut reader,
                holder.path(),
                "flathub",
                crate::flatpak::Scope::User,
                "pl_PL.UTF-8",
            )
            .unwrap();
            let player = listings
                .iter()
                .find(|item| item.id == "org.example.Player")
                .unwrap();
            assert_eq!(player.name, "Odtwarzacz");
            assert_eq!(player.summary, "Odtwarza rzeczy");
            assert_eq!(player.description, "Pierwszy akapit.");
            assert_eq!(player.screenshots[0].caption, "Odtwarzanie");
            assert_eq!(player.releases[0].notes, "Everything is faster.");
            assert_eq!(player.remote, "flathub");
            assert_eq!(player.runtime, "org.example.Platform/x86_64/25.08");
            assert_eq!(
                listings
                    .iter()
                    .find(|item| item.id == "org.example.Old")
                    .unwrap()
                    .name,
                "Still Here"
            );
        }
    }

    fn parsed() -> (tempdir::Holder, Vec<Listing>) {
        let holder = tempdir::Holder::new();
        let file = holder.path().join("appstream.xml");
        std::fs::write(&file, SAMPLE).expect("a catalogue to read");
        let icons = holder.path().join("icons");
        std::fs::create_dir_all(icons.join("128x128")).expect("an icon directory");
        std::fs::write(
            icons.join("128x128").join("org.example.Player.png"),
            [0u8; 4],
        )
        .expect("an icon");
        let listings = read_file_for(&file, &icons, "flathub", crate::flatpak::Scope::User, "en")
            .expect("the sample catalogue parses");
        (holder, listings)
    }

    #[test]
    fn only_applications_are_listed() {
        let (_holder, listings) = parsed();
        let ids: Vec<&str> = listings.iter().map(|one| one.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "org.example.Player",
                "org.example.Nameless",
                "org.example.Old"
            ],
            "a runtime was put on a shelf, or an application was left off one"
        );
    }

    #[test]
    fn an_application_is_called_what_flatpak_calls_it() {
        let (_holder, listings) = parsed();
        let old = listings
            .iter()
            .find(|one| one.name == "Still Here")
            .expect("the component with the old suffixed id");
        assert_eq!(
            old.id, "org.example.Old",
            "a component keeping the old .desktop suffix would never match \
             the application flatpak installs under that name"
        );
        assert_eq!(
            listings[0].id, "org.example.Player",
            "an id that was already right was rewritten"
        );
    }

    #[test]
    fn a_translation_never_replaces_what_it_translates() {
        let (_holder, listings) = parsed();
        let player = &listings[0];
        assert_eq!(player.name, "Player", "a translated name replaced the name");
        assert_eq!(
            player.summary, "Player is the world's most popular way to play things & more",
            "a translated summary replaced the summary"
        );
        assert!(
            !player.description.contains("Pierwszy"),
            "a translated description was gathered as well: {}",
            player.description
        );
        assert_eq!(
            player.screenshots[0].caption, "Playing something",
            "a translated caption replaced the caption"
        );
    }

    /// The one that bit: an entity breaks an element's text into pieces, and
    /// a parser that believed the first piece truncated every name, summary
    /// and description carrying an apostrophe — which on Flathub is thousands
    /// of them.
    #[test]
    fn text_broken_up_by_an_entity_is_still_gathered_whole() {
        let (_holder, listings) = parsed();
        let player = &listings[0];
        assert!(
            player.summary.ends_with("& more"),
            "an entity cut a summary short: {}",
            player.summary
        );
        assert!(
            player.summary.contains("world's"),
            "an apostrophe was lost or was left as its entity: {}",
            player.summary
        );
        assert!(
            !player.summary.contains("&apos;") && !player.summary.contains("&amp;"),
            "an entity was left unresolved: {}",
            player.summary
        );
        assert!(
            player.description.contains("René's"),
            "a numeric character reference was lost: {}",
            player.description
        );
    }

    #[test]
    fn whitespace_a_catalogue_was_written_with_is_not_part_of_the_text() {
        assert_eq!(
            tidy("  spaced  out \n  over lines ".into()),
            "spaced out over lines"
        );
        assert_eq!(tidy("already tidy".into()), "already tidy");
        assert_eq!(tidy("   ".into()), "");
        assert_eq!(
            tidy("Steam & friends".into()),
            "Steam & friends",
            "a single space between words was collapsed away"
        );
    }

    #[test]
    fn every_entity_a_catalogue_may_carry_is_resolved() {
        assert_eq!(resolve("amp"), Some('&'));
        assert_eq!(resolve("lt"), Some('<'));
        assert_eq!(resolve("gt"), Some('>'));
        assert_eq!(resolve("quot"), Some('"'));
        assert_eq!(resolve("apos"), Some('\''));
        assert_eq!(resolve("#233"), Some('é'));
        assert_eq!(resolve("#x2014"), Some('—'));
        assert_eq!(
            resolve("nbsp"),
            None,
            "an entity nothing declares was invented"
        );
    }

    #[test]
    fn an_id_this_application_stands_in_for_is_not_its_own() {
        let (_holder, listings) = parsed();
        assert_eq!(
            listings[0].id, "org.example.Player",
            "an id inside <provides> was taken for the component's own"
        );
    }

    #[test]
    fn a_description_becomes_paragraphs_with_its_list_kept() {
        let (_holder, listings) = parsed();
        assert_eq!(
            listings[0].description,
            "The first paragraph, wrapped across three lines and carrying René's name.\n\nA listed thing.",
            "a description lost a paragraph, kept its markup, or kept the \
             wrapping the catalogue happened to be written with"
        );
    }

    #[test]
    fn a_release_note_is_not_gathered_into_the_description() {
        let (_holder, listings) = parsed();
        let player = &listings[0];
        assert!(
            !player.description.contains("faster"),
            "what changed in a release was read as part of the description: {}",
            player.description
        );
        assert_eq!(
            player.releases.len(),
            2,
            "a release declared in its attributes alone was dropped"
        );
        assert_eq!(player.releases[0].version, "4.2.0");
        assert_eq!(player.releases[0].notes, "Everything is faster.");
        assert_eq!(
            player.releases[0].when,
            written(1, 1, 2026),
            "a release was dated wrongly"
        );
        assert!(
            player.releases[1].notes.is_empty(),
            "a release nothing was said about was given the note above it"
        );
    }

    /// A date in the session's language, asked for the way `day` asks for it.
    /// What the two shipped languages make of one is named in the test below.
    fn written(day: u32, month: usize, year: i64) -> String {
        crate::message!("release-date", "day" => day.to_string(),
            "month" => lxb_app::lxb_toolkit::i18n::month(month), "year" => year.to_string())
    }

    #[test]
    fn a_date_is_written_the_way_each_language_writes_one() {
        let catalog = crate::i18n::Catalog::new(crate::i18n::RESOURCES);
        let months =
            lxb_app::lxb_toolkit::i18n::Catalog::new(lxb_app::lxb_toolkit::i18n::RESOURCES);
        let said = |locale: &str| {
            let mut args = crate::i18n::FluentArgs::new();
            args.set("day", "29");
            args.set(
                "month",
                months.text_for(locale, "month-february").to_string(),
            );
            args.set("year", "2000");
            catalog.format_for(locale, "release-date", &args)
        };
        assert_eq!(said("en"), "29 February 2000");
        assert_eq!(said("pl"), "29 lutego 2000");
        // The day goes first in eight of the ten, and the month's form is the
        // language's: Russian writes the genitive as Polish does, Spanish and
        // Portuguese fence it with *de*, German points the day, and Chinese
        // goes year to day with the month's name being its number and a
        // character. A separator and three values could have written none of
        // it, which is why a date is one message.
        assert_eq!(said("de"), "29. Februar 2000");
        assert_eq!(said("es"), "29 de febrero de 2000");
        assert_eq!(said("fr"), "29 février 2000");
        assert_eq!(said("hi"), "29 फ़रवरी 2000");
        assert_eq!(said("pt_BR"), "29 de fevereiro de 2000");
        assert_eq!(said("ru"), "29 февраля 2000 г.");
        assert_eq!(said("zh_CN"), "2000年2月29日");
    }

    #[test]
    fn a_day_is_written_the_way_a_person_reads_one() {
        assert_eq!(day(0), "", "a release with no date was given one");
        assert_eq!(day(1), written(1, 1, 1970));
        assert_eq!(
            day(951_782_400),
            written(29, 2, 2000),
            "a leap day was lost"
        );
        assert_eq!(day(1_767_225_600), written(1, 1, 2026));
        assert_eq!(day(1_756_339_200), written(28, 8, 2025));
        assert_eq!(
            day(1_735_603_200),
            written(31, 12, 2024),
            "the last day of a leap year moved"
        );
    }

    #[test]
    fn only_the_links_worth_offering_are_kept() {
        let (_holder, listings) = parsed();
        let player = &listings[0];
        assert_eq!(
            player.link(Link::Homepage),
            Some("https://example.invalid/")
        );
        assert_eq!(
            player.link(Link::Bugtracker),
            Some("https://example.invalid/bugs")
        );
        assert_eq!(
            player.link(Link::Donation),
            None,
            "a link nobody published was invented"
        );
        assert_eq!(
            player.links.len(),
            2,
            "a kind of address this store has nowhere to put was kept: {:?}",
            player.links
        );
    }

    #[test]
    fn every_link_names_a_mark_the_toolkit_has() {
        for link in Link::ALL {
            assert!(
                lxb_app::lxb_toolkit::assets::glyph(link.glyph()).is_some(),
                "{} asks for a mark that is not in the toolkit: {}",
                link.title(),
                link.glyph()
            );
        }
    }

    #[test]
    fn the_widest_icon_on_the_disk_is_the_one_drawn() {
        let (holder, listings) = parsed();
        assert_eq!(
            listings[0].icon,
            Some(holder.path().join("icons/128x128/org.example.Player.png")),
            "the icon was taken at the wrong size, or from a remote address"
        );
    }

    #[test]
    fn an_icon_the_remote_promised_but_did_not_ship_is_fetched_instead() {
        let (_holder, listings) = parsed();
        assert_eq!(
            listings[1].icon, None,
            "a path to a file that is not there was kept as an icon"
        );
        assert_eq!(
            listings[0].icon_remote.as_deref(),
            Some("https://example.invalid/icon.png"),
            "the address a missing icon could be fetched from was thrown away, \
             or the doubled one beside it was taken instead"
        );
    }

    #[test]
    fn the_widest_screenshot_worth_fetching_is_kept_with_the_shape_it_has() {
        let (_holder, listings) = parsed();
        let shots = &listings[0].screenshots;
        assert_eq!(shots.len(), 1);
        assert_eq!(
            shots[0].url, "https://example.invalid/752.png",
            "a screenshot larger than it can be drawn, or smaller than it should be, was chosen"
        );
        assert_eq!(
            (shots[0].width, shots[0].height),
            (752, 423),
            "a screenshot was kept without the shape its frame has to be cut to"
        );
        let aspect = shots[0].aspect().expect("a shape");
        assert!(
            (aspect - 752.0 / 423.0).abs() < 0.001,
            "a screenshot's shape came out wrong: {aspect}"
        );
        assert_eq!(
            Shot::default().aspect(),
            None,
            "a picture that declared no size was given one anyway"
        );
    }

    #[test]
    fn what_a_listing_carries_is_what_a_page_shows() {
        let (_holder, listings) = parsed();
        let player = &listings[0];
        assert_eq!(player.developer, "An Author");
        assert_eq!(player.license, "GPL-3.0");
        assert_eq!(player.version, "4.2.0");
        assert_eq!(player.reference, "app/org.example.Player/x86_64/stable");
        assert_eq!(player.remote, "flathub");
        assert_eq!(player.categories, ["AudioVideo", "Player"]);
        assert_eq!(player.keywords, ["music"]);
        assert_eq!(
            player.runtime, "org.example.Platform/x86_64/25.08",
            "what an application runs on was not read off its bundle"
        );
        assert!(
            player.verified,
            "a remote saying the project publishes this was not believed"
        );
        assert!(
            player.rated,
            "a content rating declared with nothing in it was read as none at all"
        );
        assert!(
            !listings[1].verified,
            "an application nobody vouched for was marked as verified"
        );
    }

    #[test]
    fn an_application_with_no_name_is_called_after_its_id() {
        let (_holder, listings) = parsed();
        assert_eq!(
            listings[1].name, "Nameless",
            "an application with no name was listed with nothing to read"
        );
    }

    #[test]
    fn a_shelf_holds_what_the_shell_would_put_on_it() {
        let (_holder, listings) = parsed();
        let player = &listings[0];
        assert!(
            Section::Multimedia.holds(player),
            "AudioVideo did not land on Multimedia"
        );
        assert!(
            !Section::Games.holds(player),
            "a media player landed on the games shelf"
        );
        assert!(
            Section::Everything.holds(player),
            "Everything did not hold everything"
        );
    }

    #[test]
    fn a_search_answers_by_name_before_it_answers_by_summary() {
        let catalogue = Catalogue {
            listings: vec![
                listing_of(
                    "org.a.Drift",
                    "Drift",
                    "A video tool with an editor built in",
                ),
                listing_of("org.a.Kdenlive", "Kdenlive", "Video editor"),
                listing_of(
                    "org.a.Shot",
                    "OpenShot Video Editor",
                    "A powerful video editor",
                ),
            ],
            by_id: HashMap::new(),
        };

        let found: Vec<&str> = catalogue
            .search("video editor")
            .iter()
            .map(|one| one.name.as_str())
            .collect();
        assert_eq!(
            found,
            ["OpenShot Video Editor", "Kdenlive", "Drift"],
            "a store answered a two-word query in alphabetical order"
        );
    }

    #[test]
    fn every_word_of_a_query_has_to_appear_somewhere() {
        let catalogue = Catalogue {
            listings: vec![
                listing_of("org.a.One", "Paint", "Draws pictures"),
                listing_of("org.a.Two", "Sound", "Plays music"),
            ],
            by_id: HashMap::new(),
        };
        let found = catalogue.search("paint music");
        assert!(
            found.is_empty(),
            "a query of two words matched something carrying only one: {found:?}"
        );
        assert_eq!(
            catalogue.search("draws").len(),
            1,
            "a word in a summary was not searched"
        );
        assert!(
            catalogue.search("   ").is_empty(),
            "a query of nothing at all answered with something"
        );
    }

    fn listing_of(id: &str, name: &str, summary: &str) -> Listing {
        let mut listing = Listing {
            id: id.into(),
            name: name.into(),
            summary: summary.into(),
            reference: format!("app/{id}/x86_64/stable"),
            ..Listing::default()
        };
        listing.haystack = format!(
            "{} {} {}",
            listing.name.to_lowercase(),
            listing.summary.to_lowercase(),
            listing.id.to_lowercase()
        );
        listing
    }

    /// A directory that removes itself, so the tests need no crate for it.
    mod tempdir {
        use std::path::{Path, PathBuf};

        pub struct Holder(PathBuf);

        impl Holder {
            pub fn new() -> Self {
                let at = std::env::temp_dir().join(format!(
                    "distribumpy-test-{}-{:?}",
                    std::process::id(),
                    std::thread::current().id()
                ));
                let _ = std::fs::remove_dir_all(&at);
                std::fs::create_dir_all(&at).expect("a directory to test in");
                Self(at)
            }

            pub fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for Holder {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }
}
