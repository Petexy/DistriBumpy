//! Screenshots, which are the one part of a listing that is not already on
//! this disk.
//!
//! Everything else a store shows — names, summaries, categories, icons — comes
//! out of the catalogue flatpak already keeps, so a machine that has been
//! updated once can be browsed with the network unplugged. Screenshots are
//! addresses in that catalogue rather than files, so they are fetched once,
//! kept, and drawn from the cache ever after.
//!
//! Nothing here blocks a frame. A picture that has not arrived yet is simply
//! not drawn, and the page says so.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};

/// How long to wait for a picture nobody is watching load.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(20);

/// The largest picture worth keeping. Flathub's thumbnails are well under
/// this; the ceiling is here so that a remote which is not Flathub cannot hand
/// this store a hundred megabytes and be believed.
const MOST: u64 = 8 * 1024 * 1024;

/// What is known about one address.
#[derive(Debug, Clone)]
enum State {
    Asked,
    /// Here, and when it arrived — a picture that appeared between one frame
    /// and the next would be a picture that appeared out of nothing.
    Here(PathBuf, std::time::Instant),
    /// Nothing more will be tried for this address this run. A store that
    /// retried a 404 every frame would spend the session asking.
    Lost,
}

/// The pictures, and the thread fetching them.
pub struct Art {
    known: HashMap<String, State>,
    wanted: Sender<String>,
    arrived: Receiver<(String, Option<PathBuf>)>,
    directory: PathBuf,
}

impl Art {
    pub fn new() -> Self {
        let directory = cache();
        let _ = std::fs::create_dir_all(&directory);

        let (wanted, asked) = std::sync::mpsc::channel::<String>();
        let (voice, arrived) = std::sync::mpsc::channel::<(String, Option<PathBuf>)>();
        let where_to = directory.clone();
        std::thread::Builder::new()
            .name("distribumpy-art".into())
            .spawn(move || {
                let agent = ureq::Agent::config_builder()
                    .timeout_global(Some(PATIENCE))
                    .build()
                    .new_agent();
                while let Ok(url) = asked.recv() {
                    let got = fetch(&agent, &url, &where_to);
                    if voice.send((url, got)).is_err() {
                        break;
                    }
                }
            })
            .expect("a picture thread");

        Self {
            known: HashMap::new(),
            wanted,
            arrived,
            directory,
        }
    }

    /// Take in whatever arrived since the last frame.
    pub fn advance(&mut self) {
        for (url, got) in self.arrived.try_iter().collect::<Vec<_>>() {
            let state = match got {
                Some(path) => State::Here(path, std::time::Instant::now()),
                None => State::Lost,
            };
            self.known.insert(url, state);
        }
    }

    /// Where a picture is on this disk, asking for it if this is the first
    /// time it has been wanted.
    ///
    /// Answers `None` while it is still coming, and every frame after that if
    /// it never does — which is what a page draws its own placeholder for.
    pub fn picture(&mut self, url: &str) -> Option<PathBuf> {
        match self.known.get(url) {
            Some(State::Here(path, _)) => return Some(path.clone()),
            Some(State::Asked | State::Lost) => return None,
            None => {}
        }

        let kept = self.directory.join(name_for(url));
        if kept.is_file() {
            self.known.insert(
                url.to_string(),
                State::Here(kept.clone(), std::time::Instant::now()),
            );
            return Some(kept);
        }

        self.known.insert(url.to_string(), State::Asked);
        let _ = self.wanted.send(url.to_string());
        None
    }

    /// Whether this address is still being waited for, so a page can say
    /// "loading" rather than nothing at all.
    pub fn waiting(&self, url: &str) -> bool {
        matches!(self.known.get(url), Some(State::Asked))
    }

    /// How far in a picture is, nought to one.
    ///
    /// A picture that has just arrived comes up rather than appearing: it was
    /// fetched over a network and lands whenever it lands, and something
    /// snapping into a page in the middle of reading it is the one thing a
    /// page with pictures on it must not do.
    pub fn fade(&self, url: &str, over: f32) -> f32 {
        match self.known.get(url) {
            Some(State::Here(_, at)) => {
                lxb_app::lxb_toolkit::motion::ease((at.elapsed().as_secs_f32() / over).min(1.0))
            }
            _ => 0.0,
        }
    }

    /// Put pictures already on disk at the end of their arrival animation.
    ///
    /// Headless photographs draw two frames back-to-back. A cached picture is
    /// discovered on the first and would otherwise still be nearly invisible
    /// on the second, contradicting `--shot`'s promise of a settled page.
    pub fn settle(&mut self) {
        let arrived = std::time::Instant::now() - std::time::Duration::from_secs(1);
        for state in self.known.values_mut() {
            if let State::Here(_, at) = state {
                *at = arrived;
            }
        }
    }

    /// Ask for a picture without wanting it drawn yet.
    ///
    /// A gallery does this for the screenshots either side of the one on
    /// screen, so that stepping to the next one shows a picture rather than
    /// the words under where a picture will be.
    pub fn ask_for(&mut self, url: &str) {
        let _ = self.picture(url);
    }
}

fn fetch(agent: &ureq::Agent, url: &str, directory: &Path) -> Option<PathBuf> {
    if !url.starts_with("https://") {
        eprintln!("distribumpy: refusing a picture that is not over https: {url}");
        return None;
    }
    let mut response = match agent.get(url).call() {
        Ok(response) => response,
        Err(err) => {
            eprintln!("distribumpy: {url}: {err}");
            return None;
        }
    };
    let body = match response.body_mut().with_config().limit(MOST).read_to_vec() {
        Ok(body) => body,
        Err(err) => {
            eprintln!("distribumpy: {url}: {err}");
            return None;
        }
    };
    if body.is_empty() {
        return None;
    }

    let path = directory.join(name_for(url));
    // Written beside and moved into place, so a picture interrupted halfway
    // is never left where the next run would find it and believe it.
    let part = path.with_extension("part");
    if std::fs::write(&part, &body).is_err() {
        return None;
    }
    if std::fs::rename(&part, &path).is_err() {
        let _ = std::fs::remove_file(&part);
        return None;
    }
    Some(path)
}

/// A stable file name for an address.
///
/// The address itself cannot be one — it is far longer than a file name may be
/// and carries separators — so this is its hash, which is all a cache needs.
fn name_for(url: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in url.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let kind = if url.ends_with(".jpg") || url.ends_with(".jpeg") {
        "jpg"
    } else {
        "png"
    };
    format!("{hash:016x}.{kind}")
}

fn cache() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("distribumpy")
        .join("screenshots")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_address_always_gets_the_same_file_name() {
        let url = "https://dl.flathub.org/media/org/videolan/VLC/x/screenshots/image-1_752x423.png";
        assert_eq!(name_for(url), name_for(url), "a cache name was not stable");
        assert_ne!(
            name_for(url),
            name_for(&url.replace("image-1", "image-2")),
            "two screenshots of one application were given one file"
        );
    }

    #[test]
    fn a_cache_name_is_a_file_name_and_not_a_path() {
        let name = name_for("https://example.invalid/a/b/c.png");
        assert!(
            !name.contains('/') && !name.contains(".."),
            "a cache name could leave the cache directory: {name}"
        );
        assert!(
            name.ends_with(".png"),
            "a picture was not named for what it is: {name}"
        );
    }

    #[test]
    fn a_jpeg_keeps_its_own_kind() {
        assert!(
            name_for("https://example.invalid/shot.jpg").ends_with(".jpg"),
            "a jpeg was named a png, which is what the decoder would then expect"
        );
    }
}
