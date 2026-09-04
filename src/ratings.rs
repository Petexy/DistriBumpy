//! What the people who have used these applications think of them.
//!
//! **Flathub publishes no ratings** — see `src/flathub.rs` — so these do not
//! come from the remote the applications do. They come from the Open Desktop
//! Ratings Service, which is the same place GNOME Software and Plasma
//! Discover get theirs: one address, one answer, every application at once.
//!
//! What it hands back is a count of one-star through five-star reviews per
//! application, and nothing else. There is no prose here and no way to leave
//! a review from this store: writing one is a conversation with a service
//! this store would then have to be an account on, and reading somebody's
//! paragraph is a thing a shelf of cards has no room for. What is worth
//! having is the two numbers, because they are two of the ways somebody wants
//! a long shelf put in order.
//!
//! It is kept on this disk after it is fetched, so a machine with no network
//! sorts by whatever it last knew rather than by nothing.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

/// Where the answer comes from. The same address Discover asks.
const WHERE: &str = "https://odrs.gnome.org/1.0/reviews/api/ratings";

/// How long an answer stays worth believing before it is fetched again.
///
/// A day: reviews arrive at the rate people write them, which is nothing like
/// the rate a store is opened.
const KEEPS: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// The most the answer may weigh. It is about two megabytes for every
/// application anybody has ever reviewed; the ceiling is here so that an
/// address which is not that one cannot hand this store a hundred megabytes
/// and be believed.
const MOST: u64 = 16 * 1024 * 1024;

/// How many reviews a rating is worth believing on its own.
///
/// One five-star review is not a better application than four hundred at four
/// and a half, and a shelf sorted as though it were is a shelf sorted by who
/// has the fewest opinions about them. So a rating is pulled towards the
/// middle by this many imagined three-star reviews before anything is put in
/// order by it — a plain Bayesian average, and the same shape of correction
/// every store that ranks by rating has to make somewhere.
const DOUBT: f32 = 12.0;

/// What people said about one application.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rating {
    /// How many gave it one star, two, and so on to five.
    pub stars: [u32; 5],
}

impl Rating {
    pub fn reviews(self) -> u32 {
        self.stars.iter().sum()
    }

    /// The plain average, which is the number worth showing somebody.
    pub fn mean(self) -> f32 {
        let reviews = self.reviews();
        if reviews == 0 {
            return 0.0;
        }
        let sum: u32 = self
            .stars
            .iter()
            .enumerate()
            .map(|(at, count)| (at as u32 + 1) * count)
            .sum();
        sum as f32 / reviews as f32
    }

    /// The number worth putting a shelf in order by, which is not the same
    /// one. See [`DOUBT`].
    pub fn score(self) -> f32 {
        let reviews = self.reviews() as f32;
        if reviews <= 0.0 {
            return 0.0;
        }
        (self.mean() * reviews + 3.0 * DOUBT) / (reviews + DOUBT)
    }
}

/// Every rating this machine knows, and the thread fetching them.
pub struct Ratings {
    known: HashMap<String, Rating>,
    arrived: Receiver<HashMap<String, Rating>>,
    asked: bool,
    /// Set when an answer arrived since the last frame, so whatever was built
    /// out of these can be built again.
    pub changed: bool,
}

impl Ratings {
    pub fn new() -> Self {
        let file = cache();
        let _ = file.parent().map(std::fs::create_dir_all);

        let (voice, arrived) = std::sync::mpsc::channel::<HashMap<String, Rating>>();
        let known = read(&file).unwrap_or_default();
        let asked = stale(&file);
        if asked {
            let where_to = file.clone();
            std::thread::Builder::new()
                .name("distribumpy-ratings".into())
                .spawn(move || {
                    let agent = ureq::Agent::config_builder()
                        .timeout_global(Some(std::time::Duration::from_secs(30)))
                        .build()
                        .new_agent();
                    let _ = voice.send(fetch(&agent, &where_to));
                })
                .expect("a ratings thread");
        }

        Self {
            known,
            arrived,
            asked,
            changed: false,
        }
    }

    /// Nothing, for a run that must not touch the network.
    pub fn none() -> Self {
        let (_, arrived) = std::sync::mpsc::channel();
        Self {
            known: HashMap::new(),
            arrived,
            asked: false,
            changed: false,
        }
    }

    /// Take in whatever arrived since the last frame.
    pub fn advance(&mut self) {
        self.changed = false;
        for said in self.arrived.try_iter().collect::<Vec<_>>() {
            self.asked = false;
            // Nothing came back. Whatever is already here stays here: an old
            // answer is worth more than none.
            if said.is_empty() {
                continue;
            }
            self.known = said;
            self.changed = true;
        }
    }

    /// What was said about one application.
    ///
    /// The service keys some of them by the desktop entry and some by the
    /// application, and there is no telling which from here — VLC is
    /// `org.videolan.VLC` and the colour picker is `nl.hjdskes.gcolor3.desktop`
    /// — so both are asked for.
    pub fn of(&self, id: &str) -> Option<Rating> {
        self.known
            .get(id)
            .or_else(|| self.known.get(&format!("{id}.desktop")))
            .copied()
    }

    /// Whether anything at all is known, which is what decides whether an
    /// order that reads these is worth offering.
    pub fn any(&self) -> bool {
        !self.known.is_empty()
    }

    /// Whether an answer is still on its way. It stops being on its way when
    /// one arrives, whether or not there was anything in it, so nothing waits
    /// on this for ever with the network down.
    pub fn waiting(&self) -> bool {
        self.asked
    }
}

impl Default for Ratings {
    fn default() -> Self {
        Self::none()
    }
}

fn fetch(agent: &ureq::Agent, file: &std::path::Path) -> HashMap<String, Rating> {
    let mut response = match agent.get(WHERE).call() {
        Ok(response) => response,
        Err(err) => {
            eprintln!("distribumpy: {WHERE}: {err}");
            return HashMap::new();
        }
    };
    let body = match response
        .body_mut()
        .with_config()
        .limit(MOST)
        .read_to_string()
    {
        Ok(body) => body,
        Err(err) => {
            eprintln!("distribumpy: {WHERE}: {err}");
            return HashMap::new();
        }
    };
    let found = ratings_in(&body);
    if found.is_empty() {
        return found;
    }
    write(file, &found);
    found
}

/// Every application in an answer.
///
/// Read out of the JSON rather than through a parser, for the same reason the
/// collections are: two numbers of it are wanted and the rest is counts of a
/// star nobody gave. An application is a quoted name followed by an object —
/// which is what tells one apart from the `"star1"` inside one, since those
/// are followed by a number.
fn ratings_in(body: &str) -> HashMap<String, Rating> {
    let mut found = HashMap::new();
    let mut rest = body;
    while let Some(open) = rest.find('"') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('"') else { break };
        let name = &rest[..close];
        rest = &rest[close + 1..];

        let after = rest.trim_start();
        let Some(after) = after.strip_prefix(':') else {
            continue;
        };
        let after = after.trim_start();
        let Some(inside) = after.strip_prefix('{') else {
            continue;
        };
        let Some(end) = inside.find('}') else { break };
        let rating = stars_in(&inside[..end]);
        let name = name.to_string();
        rest = &inside[end + 1..];

        if rating.reviews() > 0 && looks_like_an_id(&name) {
            found.insert(name, rating);
        }
    }
    found
}

/// The five counts inside one application's object.
fn stars_in(inside: &str) -> Rating {
    let mut stars = [0u32; 5];
    for (at, star) in stars.iter_mut().enumerate() {
        let key = format!("\"star{}\"", at + 1);
        let Some(found) = inside.find(&key) else {
            continue;
        };
        let after = inside[found + key.len()..].trim_start();
        let Some(after) = after.strip_prefix(':') else {
            continue;
        };
        let digits: String = after
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        *star = digits.parse().unwrap_or(0);
    }
    Rating { stars }
}

/// Whether a string could be what flatpak calls an application, or the desktop
/// entry of one. The service is a list of everything anybody has reviewed on
/// any desktop, and most of it is not from a repository this store has.
fn looks_like_an_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() < 256
        && id.contains('.')
        && id
            .chars()
            .all(|letter| letter.is_ascii_alphanumeric() || matches!(letter, '.' | '-' | '_'))
}

/// One line per application: its name and its five counts.
///
/// Written rather than the answer itself, because the answer is two megabytes
/// of which this is the tenth that is read.
fn write(file: &std::path::Path, found: &HashMap<String, Rating>) {
    let mut lines = String::with_capacity(found.len() * 40);
    for (name, rating) in found {
        lines.push_str(name);
        for star in rating.stars {
            lines.push(' ');
            lines.push_str(&star.to_string());
        }
        lines.push('\n');
    }
    let part = file.with_extension("part");
    if std::fs::write(&part, lines).is_ok() && std::fs::rename(&part, file).is_err() {
        let _ = std::fs::remove_file(&part);
    }
}

fn read(file: &std::path::Path) -> Option<HashMap<String, Rating>> {
    let text = std::fs::read_to_string(file).ok()?;
    let mut found = HashMap::new();
    for line in text.lines() {
        let mut said = line.split(' ');
        let Some(name) = said.next().filter(|name| looks_like_an_id(name)) else {
            continue;
        };
        let mut stars = [0u32; 5];
        let mut all = true;
        for star in stars.iter_mut() {
            match said.next().and_then(|count| count.parse().ok()) {
                Some(count) => *star = count,
                None => all = false,
            }
        }
        if all {
            found.insert(name.to_string(), Rating { stars });
        }
    }
    (!found.is_empty()).then_some(found)
}

/// Whether the answer on the disk is old enough to be worth asking about
/// again. One that was never fetched at all is as stale as one can be.
fn stale(file: &std::path::Path) -> bool {
    let Ok(written) = std::fs::metadata(file).and_then(|about| about.modified()) else {
        return true;
    };
    written.elapsed().map(|age| age > KEEPS).unwrap_or(true)
}

fn cache() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("distribumpy")
        .join("ratings.list")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shaped the way the service really answers, down to the star nobody
    /// gave and the entries that are not applications this store could show.
    const ANSWER: &str = r#"{
        "0ad.desktop": {"star0": 0, "star1": 3, "star2": 1, "star3": 0, "star4": 6, "star5": 17, "total": 27},
        "org.videolan.VLC": {"star0": 0, "star1": 311, "star2": 109, "star3": 97, "star4": 107, "star5": 856, "total": 1480},
        "Microbit": {"star0": 0, "star1": 1, "star2": 0, "star3": 0, "star4": 0, "star5": 0, "total": 1},
        "org.example.Unreviewed": {"star0": 0, "star1": 0, "star2": 0, "star3": 0, "star4": 0, "star5": 0, "total": 0}
    }"#;

    #[test]
    fn an_answer_is_read_by_application_rather_than_by_its_stars() {
        let found = ratings_in(ANSWER);
        assert_eq!(
            found.get("0ad.desktop").map(|one| one.stars),
            Some([3, 1, 0, 6, 17]),
            "the counts inside an application were read as applications, or \
             not read at all"
        );
        assert_eq!(
            found.get("org.videolan.VLC").map(|one| one.reviews()),
            Some(1480)
        );
        assert!(
            !found.contains_key("Microbit"),
            "something that is not an application id was kept"
        );
        assert!(
            !found.contains_key("org.example.Unreviewed"),
            "an application nobody has reviewed was kept as a rating of none"
        );
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn an_answer_that_is_not_one_leaves_nothing_rather_than_something_wrong() {
        assert!(ratings_in("").is_empty());
        assert!(ratings_in("{\"detail\": \"Not Found\"}").is_empty());
        assert!(
            ratings_in("{\"org.example.A\": {").is_empty(),
            "an answer cut off mid-object was read past its end"
        );
        assert!(
            ratings_in("{\"org.example.A\"").is_empty(),
            "an answer cut off after a name was read past its end"
        );
    }

    #[test]
    fn the_average_is_what_people_gave_it() {
        let one = Rating {
            stars: [0, 0, 0, 0, 4],
        };
        assert!((one.mean() - 5.0).abs() < 0.001);
        let mixed = Rating {
            stars: [1, 0, 0, 0, 1],
        };
        assert!((mixed.mean() - 3.0).abs() < 0.001);
        assert_eq!(Rating::default().mean(), 0.0, "nothing said is not nought");
    }

    #[test]
    fn a_shelf_is_not_ordered_by_who_has_the_fewest_opinions_about_them() {
        let lonely = Rating {
            stars: [0, 0, 0, 0, 1],
        };
        let loved = Rating {
            stars: [4, 2, 8, 90, 400],
        };
        assert!(lonely.mean() > loved.mean(), "the averages say otherwise");
        assert!(
            loved.score() > lonely.score(),
            "one five-star review outranked four hundred: {} against {}",
            lonely.score(),
            loved.score()
        );
        assert!(
            loved.score() <= loved.mean(),
            "doubt made a rating better than it was"
        );
    }

    #[test]
    fn what_is_written_is_what_is_read_back() {
        let mut found = HashMap::new();
        found.insert(
            "org.example.A".to_string(),
            Rating {
                stars: [1, 2, 3, 4, 5],
            },
        );
        let file =
            std::env::temp_dir().join(format!("distribumpy-ratings-{}.list", std::process::id()));
        write(&file, &found);
        let back = read(&file).expect("what was written came back as nothing");
        let _ = std::fs::remove_file(&file);
        assert_eq!(back, found);
    }
}
