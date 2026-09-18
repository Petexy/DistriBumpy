//! What Flathub itself says is worth looking at.
//!
//! Everything else this store shows comes off the disk. These four lists
//! cannot: what is popular this month, what is rising, and what has just been
//! published or rebuilt are facts about everybody else's machines, and only
//! the people running the repository know them.
//!
//! **Flathub publishes no ratings.** There is no score, no stars and no
//! reviews in its API, so nothing here invents one. What it does publish is
//! how much a thing is being installed (`popular`) and how sharply that is
//! rising (`trending`), and those are the two honest answers to "what is
//! everybody else using".
//!
//! Each list is kept on this disk after it is fetched, so the Home page comes
//! up filled on a machine with no network — with lists that are as old as the
//! last time there was one, which is the truthful thing to do with a fact
//! about somewhere else.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

/// How long a list stays worth believing before it is fetched again.
///
/// What is popular does not change in an afternoon, and a store that asked
/// four times an hour would be spending somebody's network on nothing.
const KEEPS: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

/// How many of each list to ask for.
///
/// Two lines of a three-wide grid. Asking for more would fetch a page of
/// descriptions nothing here reads — the catalogue on this disk already has
/// every word about every one of them.
const HOW_MANY: usize = 6;

/// The most a list may weigh. Flathub's answer for six applications is about
/// eleven kilobytes; the ceiling is here so that an address which is not
/// Flathub cannot hand this store a hundred megabytes and be believed.
const MOST: u64 = 4 * 1024 * 1024;

/// One of Flathub's own lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Collection {
    Popular,
    Trending,
    New,
    Updated,
}

impl Collection {
    /// In the order the Home page shows them.
    pub const ALL: [Collection; 4] = [
        Collection::Popular,
        Collection::Trending,
        Collection::New,
        Collection::Updated,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Collection::Popular => crate::i18n::text("popular-this-month"),
            Collection::Trending => crate::i18n::text("rising-fastest"),
            Collection::New => crate::i18n::text("just-published"),
            Collection::Updated => crate::i18n::text("just-updated"),
        }
    }

    /// What the list is, said once under the heading.
    pub fn note(self) -> &'static str {
        match self {
            Collection::Popular => crate::i18n::text("popular-this-month-note"),
            Collection::Trending => crate::i18n::text("rising-fastest-note"),
            Collection::New => crate::i18n::text("just-published-note"),
            Collection::Updated => crate::i18n::text("just-updated-note"),
        }
    }

    fn path(self) -> &'static str {
        match self {
            Collection::Popular => "popular",
            Collection::Trending => "trending",
            Collection::New => "recently-added",
            Collection::Updated => "recently-updated",
        }
    }

    fn url(self) -> String {
        format!(
            "https://flathub.org/api/v2/collection/{}?page=1&per_page={HOW_MANY}",
            self.path()
        )
    }
}

/// Flathub's lists, and the thread fetching them.
pub struct Collections {
    known: HashMap<Collection, Vec<String>>,
    wanted: Sender<Collection>,
    arrived: Receiver<(Collection, Vec<String>)>,
    /// How many are still being waited for, so a page can say it is fetching
    /// rather than say nothing at all.
    asked: usize,
    /// Set when something arrived since the last frame, so that whatever is
    /// built out of these can be built again.
    pub changed: bool,
}

impl Collections {
    pub fn new() -> Self {
        let directory = cache();
        let _ = std::fs::create_dir_all(&directory);

        let (wanted, asked) = std::sync::mpsc::channel::<Collection>();
        let (voice, arrived) = std::sync::mpsc::channel::<(Collection, Vec<String>)>();
        let where_to = directory.clone();
        std::thread::Builder::new()
            .name("distribumpy-flathub".into())
            .spawn(move || {
                let agent = ureq::Agent::config_builder()
                    .timeout_global(Some(std::time::Duration::from_secs(20)))
                    .build()
                    .new_agent();
                while let Ok(which) = asked.recv() {
                    let got = fetch(&agent, which, &where_to);
                    if voice.send((which, got)).is_err() {
                        break;
                    }
                }
            })
            .expect("a collections thread");

        let mut collections = Self {
            known: HashMap::new(),
            wanted,
            arrived,
            asked: 0,
            changed: false,
        };

        // Whatever is on the disk, straight away — the Home page comes up
        // filled while anything newer is still on its way.
        for which in Collection::ALL {
            let file = directory.join(format!("{}.list", which.path()));
            if let Some(ids) = read(&file) {
                collections.known.insert(which, ids);
            }
            if stale(&file) {
                collections.asked += 1;
                let _ = collections.wanted.send(which);
            }
        }
        collections
    }

    /// Take in whatever arrived since the last frame.
    pub fn advance(&mut self) {
        self.changed = false;
        for (which, ids) in self.arrived.try_iter().collect::<Vec<_>>() {
            self.asked = self.asked.saturating_sub(1);
            if ids.is_empty() {
                // Nothing came back. Whatever is already here stays here: an
                // old list is worth more than an empty page.
                continue;
            }
            self.known.insert(which, ids);
            self.changed = true;
        }
    }

    pub fn ids(&self, which: Collection) -> &[String] {
        self.known.get(&which).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Whether anything at all is known, which is what decides between a Home
    /// page and a Home page saying why it is empty.
    pub fn any(&self) -> bool {
        self.known.values().any(|ids| !ids.is_empty())
    }

    /// Whether the network is still being waited on.
    pub fn waiting(&self) -> bool {
        self.asked > 0
    }
}

fn fetch(agent: &ureq::Agent, which: Collection, directory: &std::path::Path) -> Vec<String> {
    let url = which.url();
    let mut response = match agent.get(&url).call() {
        Ok(response) => response,
        Err(err) => {
            eprintln!("distribumpy: {url}: {err}");
            return Vec::new();
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
            eprintln!("distribumpy: {url}: {err}");
            return Vec::new();
        }
    };
    let ids = ids_in(&body);
    if ids.is_empty() {
        return ids;
    }

    let file = directory.join(format!("{}.list", which.path()));
    let part = file.with_extension("part");
    if std::fs::write(&part, ids.join("\n")).is_ok() && std::fs::rename(&part, &file).is_err() {
        let _ = std::fs::remove_file(&part);
    }
    ids
}

/// Every application id in an answer, in the order it was answered.
///
/// Read out of the JSON rather than through a parser, because one field of it
/// is wanted and the rest of the answer is the descriptions this store already
/// has better copies of on the disk. An id that is not an id is dropped, and
/// an id nothing on this machine offers is dropped later, when it is looked
/// up — so the worst a changed answer can do is leave a list short.
fn ids_in(body: &str) -> Vec<String> {
    const KEY: &str = "\"app_id\"";
    let mut found = Vec::new();
    let mut rest = body;
    while let Some(at) = rest.find(KEY) {
        rest = &rest[at + KEY.len()..];
        let Some(colon) = rest.find(':') else { break };
        let after = rest[colon + 1..].trim_start();
        let Some(quoted) = after.strip_prefix('"') else {
            continue;
        };
        let Some(end) = quoted.find('"') else { break };
        let id = &quoted[..end];
        if looks_like_an_id(id) && !found.iter().any(|known| known == id) {
            found.push(id.to_string());
        }
    }
    found
}

/// Whether a string could be what flatpak calls an application.
fn looks_like_an_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() < 256
        && id.contains('.')
        && id
            .chars()
            .all(|letter| letter.is_ascii_alphanumeric() || matches!(letter, '.' | '-' | '_'))
}

fn read(file: &std::path::Path) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(file).ok()?;
    let ids: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|line| looks_like_an_id(line))
        .map(str::to_string)
        .collect();
    (!ids.is_empty()).then_some(ids)
}

/// Whether a list is old enough to be worth asking about again. A list that
/// was never fetched at all is as stale as one can be.
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
        .join("collections")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shaped the way Flathub really answers, down to the description that
    /// carries braces and quotes of its own.
    const ANSWER: &str = r#"{"hits":[
      {"name":"Sober","summary":"Play, chat & explore","description":"Not \"app_id\": affiliated",
       "id":"org_vinegarhq_Sober","app_id":"org.vinegarhq.Sober","type":"desktop-application"},
      {"name":"Firefox","app_id" : "org.mozilla.firefox","icon":"https://example.invalid/a.png"},
      {"name":"Again","app_id":"org.mozilla.firefox"},
      {"name":"Broken","app_id":"not an id"},
      {"name":"Also broken","app_id":null}
    ],"page":1,"totalHits":3315}"#;

    #[test]
    fn every_application_in_an_answer_is_read_in_the_order_it_was_answered() {
        assert_eq!(
            ids_in(ANSWER),
            ["org.vinegarhq.Sober", "org.mozilla.firefox"],
            "a list came back in the wrong order, short, or with something \
             that is not an application in it"
        );
    }

    #[test]
    fn an_answer_that_is_not_one_leaves_a_list_empty_rather_than_wrong() {
        assert!(ids_in("").is_empty());
        assert!(ids_in("{\"detail\":\"Not Found\"}").is_empty());
        assert!(
            ids_in("{\"app_id\":").is_empty(),
            "an answer cut off mid-field was read past its end"
        );
        assert!(
            ids_in("{\"app_id\":\"").is_empty(),
            "an answer cut off mid-value was read past its end"
        );
    }

    #[test]
    fn what_is_and_is_not_an_application_id() {
        assert!(looks_like_an_id("org.videolan.VLC"));
        assert!(looks_like_an_id("io.github.some-body.A_Thing"));
        assert!(!looks_like_an_id("VLC"), "a name with no dot in it");
        assert!(!looks_like_an_id(""), "nothing at all");
        assert!(!looks_like_an_id("../../etc/passwd"), "a path");
        assert!(!looks_like_an_id("org.a b.C"), "a space");
    }

    #[test]
    fn every_list_is_asked_for_over_https_and_named_for_a_file() {
        for which in Collection::ALL {
            let url = which.url();
            assert!(
                url.starts_with("https://flathub.org/api/v2/collection/"),
                "{} is fetched from somewhere unexpected: {url}",
                which.title()
            );
            assert!(!which.title().is_empty() && !which.note().is_empty());
            assert!(
                !which.path().contains('/') && !which.path().contains(".."),
                "{} would be cached outside the cache: {}",
                which.title(),
                which.path()
            );
        }
        assert_eq!(
            Collection::ALL
                .iter()
                .map(|which| which.path())
                .collect::<std::collections::HashSet<_>>()
                .len(),
            Collection::ALL.len(),
            "two lists would be cached in one file"
        );
    }

    #[test]
    fn a_list_that_was_never_fetched_is_as_stale_as_one_can_be() {
        assert!(stale(&cache().join("nothing-was-ever-written-here.list")));
    }
}
