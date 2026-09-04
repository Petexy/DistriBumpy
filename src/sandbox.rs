//! What an application is allowed to reach outside its sandbox, and how to
//! change it.
//!
//! Two files decide this together. The application's own **metadata**, written
//! by whoever built it, is what it asks for; it lives inside the installation
//! and nothing here ever writes to it. The user's **override**, in
//! `~/.local/share/flatpak/overrides/<id>`, is what this machine's owner has
//! since said about it, and it is the only file this store edits.
//!
//! Overrides are written as differences rather than as a whole permission set:
//! `network` turns something on that the application did not ask for, and
//! `!network` turns off something it did. Setting a permission back to what
//! the application asked for removes the entry entirely, so an override file
//! only ever says what somebody actually changed — which is what makes it safe
//! to keep across an update that changes what the application asks for.
//!
//! The user's override applies to an application installed system-wide just as
//! it does to one installed for the user, so nothing here needs a password.

use std::path::PathBuf;

/// Which part of a sandbox a permission belongs to, which is also the keyfile
/// key it is written under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Share,
    Socket,
    Device,
    Feature,
    Filesystem,
}

impl Group {
    pub fn title(self) -> &'static str {
        match self {
            Group::Share => "Sharing",
            Group::Socket => "Sockets",
            Group::Device => "Devices",
            Group::Feature => "Features",
            Group::Filesystem => "Files",
        }
    }

    /// The key inside `[Context]` this group is written under.
    fn key(self) -> &'static str {
        match self {
            Group::Share => "shared",
            Group::Socket => "sockets",
            Group::Device => "devices",
            Group::Feature => "features",
            Group::Filesystem => "filesystems",
        }
    }

    /// Every key that carries a permission, in the order a page shows them.
    const ALL: [Group; 5] = [
        Group::Share,
        Group::Socket,
        Group::Device,
        Group::Feature,
        Group::Filesystem,
    ];
}

/// One permission somebody can turn on or off.
#[derive(Debug, Clone, Copy)]
pub struct Toggle {
    pub group: Group,
    /// What flatpak calls it, which is what goes in the file.
    pub key: &'static str,
    /// What a person calls it.
    pub title: &'static str,
    /// What it actually lets the application do, said plainly. A permissions
    /// page nobody understands is a permissions page nobody uses.
    pub note: &'static str,
}

/// The permissions worth putting in front of somebody.
///
/// Not every permission flatpak understands: a store that listed all of them
/// would bury the four that matter under twenty that do not. Anything left off
/// this list is still shown, as a line of text under **Also asked for**, so
/// nothing an application asked for is ever hidden — it simply cannot be
/// toggled here.
pub const TOGGLES: &[Toggle] = &[
    Toggle {
        group: Group::Share,
        key: "network",
        title: "Network",
        note: "Reach the internet and the local network",
    },
    Toggle {
        group: Group::Share,
        key: "ipc",
        title: "Talk to the display server directly",
        note: "Shared memory with the windowing system, which makes drawing faster",
    },
    Toggle {
        group: Group::Socket,
        key: "wayland",
        title: "Show windows",
        note: "Draw on this display through Wayland",
    },
    Toggle {
        group: Group::Socket,
        key: "fallback-x11",
        title: "Show windows the old way",
        note: "Use X11 where Wayland is not available",
    },
    Toggle {
        group: Group::Socket,
        key: "x11",
        title: "Full X11 access",
        note: "Every X11 client can watch every other one, including what is typed",
    },
    Toggle {
        group: Group::Socket,
        key: "pulseaudio",
        title: "Sound",
        note: "Play sound, and record it",
    },
    Toggle {
        group: Group::Socket,
        key: "session-bus",
        title: "Full session bus access",
        note: "Talk to everything this user is running, around the portals",
    },
    Toggle {
        group: Group::Socket,
        key: "system-bus",
        title: "Full system bus access",
        note: "Talk to the services running for the whole machine",
    },
    Toggle {
        group: Group::Socket,
        key: "ssh-auth",
        title: "SSH keys",
        note: "Use the keys held by this session's SSH agent",
    },
    Toggle {
        group: Group::Socket,
        key: "cups",
        title: "Printing",
        note: "Reach the printers this machine knows about",
    },
    Toggle {
        group: Group::Device,
        key: "dri",
        title: "Graphics",
        note: "Use the graphics card, which anything drawing quickly needs",
    },
    Toggle {
        group: Group::Device,
        key: "input",
        title: "Controllers",
        note: "Read gamepads and other input devices directly",
    },
    Toggle {
        group: Group::Device,
        key: "usb",
        title: "USB devices",
        note: "Talk to devices plugged into this machine",
    },
    Toggle {
        group: Group::Device,
        key: "all",
        title: "Every device",
        note: "Everything in /dev, which is more than any application needs",
    },
    Toggle {
        group: Group::Feature,
        key: "devel",
        title: "Debugging",
        note: "Use the system calls a debugger needs",
    },
    Toggle {
        group: Group::Feature,
        key: "bluetooth",
        title: "Bluetooth",
        note: "Talk to Bluetooth devices directly",
    },
    Toggle {
        group: Group::Feature,
        key: "multiarch",
        title: "32-bit code",
        note: "Run programs built for the other architecture",
    },
    Toggle {
        group: Group::Filesystem,
        key: "home",
        title: "All your files",
        note: "Read and write everything in your home folder, not only what you open",
    },
    Toggle {
        group: Group::Filesystem,
        key: "host",
        title: "All system files",
        note: "Read and write the whole machine outside the sandbox",
    },
    Toggle {
        group: Group::Filesystem,
        key: "xdg-download",
        title: "Downloads",
        note: "Read and write your Downloads folder",
    },
];

/// Where a permission stands, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// The application asked for it and nobody has said otherwise.
    Asked,
    /// The application did not ask for it and nobody has said otherwise.
    NotAsked,
    /// Turned on here, over what the application asked for.
    TurnedOn,
    /// Turned off here, over what the application asked for.
    TurnedOff,
}

impl Standing {
    pub fn on(self) -> bool {
        matches!(self, Standing::Asked | Standing::TurnedOn)
    }

    /// Whether this machine's owner is the reason it stands where it does.
    pub fn changed(self) -> bool {
        matches!(self, Standing::TurnedOn | Standing::TurnedOff)
    }
}

/// One application's sandbox, as it will actually run.
#[derive(Debug, Clone, Default)]
pub struct Sandbox {
    /// What the application asked for, per group.
    asked: Vec<(Group, Vec<String>)>,
    /// What the override says, per group, `!name` and all.
    said: Vec<(Group, Vec<String>)>,
}

impl Sandbox {
    /// Read an application's own metadata and this user's override for it.
    ///
    /// `metadata` is the file flatpak keeps beside the application, handed
    /// over as text by the caller — reading it goes through libflatpak, and
    /// nothing in this file talks to libflatpak.
    pub fn read(metadata: &str, id: &str) -> Self {
        let mut sandbox = Self {
            asked: lists_in(metadata),
            said: Vec::new(),
        };
        if let Ok(text) = std::fs::read_to_string(overrides_for(id)) {
            sandbox.said = lists_in(&text);
        }
        sandbox
    }

    /// Where one permission stands.
    pub fn standing(&self, toggle: &Toggle) -> Standing {
        let said = self.list(&self.said, toggle.group);
        let asked = self
            .list(&self.asked, toggle.group)
            .iter()
            .any(|one| names(one) == toggle.key);

        if said.iter().any(|one| one == &format!("!{}", toggle.key)) {
            return Standing::TurnedOff;
        }
        if said.iter().any(|one| names(one) == toggle.key) {
            // An override repeating what the application already asked for is
            // not a change anybody made on purpose; it is what flatpak writes
            // when something else in the same group was changed.
            return if asked {
                Standing::Asked
            } else {
                Standing::TurnedOn
            };
        }
        if asked {
            Standing::Asked
        } else {
            Standing::NotAsked
        }
    }

    /// Everything an application asked for that this store has no switch for,
    /// so that nothing it asked for is ever hidden.
    pub fn also_asked(&self) -> Vec<String> {
        let mut rest = Vec::new();
        for (group, list) in &self.asked {
            for one in list {
                let named = names(one);
                if TOGGLES
                    .iter()
                    .any(|toggle| toggle.group == *group && toggle.key == named)
                {
                    continue;
                }
                rest.push(one.clone());
            }
        }
        rest.sort();
        rest.dedup();
        rest
    }

    /// Whether this machine's owner has changed anything at all.
    pub fn touched(&self) -> bool {
        TOGGLES.iter().any(|toggle| self.standing(toggle).changed())
    }

    fn list(&self, from: &[(Group, Vec<String>)], group: Group) -> Vec<String> {
        from.iter()
            .find(|(one, _)| *one == group)
            .map(|(_, list)| list.clone())
            .unwrap_or_default()
    }
}

/// Turn one permission on or off, writing the user's override file.
///
/// Setting a permission back to what the application asked for takes the entry
/// out again rather than writing the opposite of it, so an override file only
/// ever carries what somebody actually changed.
pub fn set(sandbox: &Sandbox, id: &str, toggle: &Toggle, on: bool) -> Result<(), String> {
    let asked = sandbox
        .list(&sandbox.asked, toggle.group)
        .iter()
        .any(|one| names(one) == toggle.key);

    let mut said = sandbox.list(&sandbox.said, toggle.group);
    said.retain(|one| names(one) != toggle.key);
    if on != asked {
        said.push(if on {
            toggle.key.to_string()
        } else {
            format!("!{}", toggle.key)
        });
    }
    said.sort();

    let path = overrides_for(id);
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let written = rewrite(&existing, toggle.group, &said);

    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)
            .map_err(|err| format!("{}: {err}", directory.display()))?;
    }
    // Written beside and moved into place: an override file half written is
    // one flatpak would refuse to start the application with.
    let part = path.with_extension("part");
    std::fs::write(&part, written).map_err(|err| format!("{}: {err}", part.display()))?;
    std::fs::rename(&part, &path).map_err(|err| {
        let _ = std::fs::remove_file(&part);
        format!("{}: {err}", path.display())
    })
}

/// Take back every change made here, leaving the application asking for what
/// it always asked for.
pub fn forget(id: &str) -> Result<(), String> {
    let path = overrides_for(id);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!("{}: {err}", path.display())),
    }
}

/// `~/.local/share/flatpak/overrides/<id>`, honouring `XDG_DATA_HOME`.
///
/// The user's own overrides, always — they apply to an application installed
/// for the whole machine exactly as they do to one installed for this user,
/// and writing them needs no password. The system's overrides live somewhere
/// only root can write, and this store does not touch them.
pub fn overrides_for(id: &str) -> PathBuf {
    data_home().join("flatpak").join("overrides").join(id)
}

fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
        })
        .unwrap_or_else(std::env::temp_dir)
}

/// A permission with its mode taken off: `home:ro` is still `home`, and
/// `!network` is still `network`.
fn names(entry: &str) -> &str {
    entry
        .trim_start_matches('!')
        .split(':')
        .next()
        .unwrap_or(entry)
}

/// Every permission list in a `[Context]` section.
///
/// A very small keyfile reader, because that is all this needs: one known
/// section, five known keys, and semicolon-separated values. Nothing else in
/// the file is read, and a file with no `[Context]` at all reads as an
/// application that asked for nothing.
fn lists_in(text: &str) -> Vec<(Group, Vec<String>)> {
    let mut found = Vec::new();
    let mut in_context = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_context = line == "[Context]";
            continue;
        }
        if !in_context {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let Some(group) = Group::ALL.into_iter().find(|one| one.key() == key) else {
            continue;
        };
        let list: Vec<String> = value
            .split(';')
            .map(str::trim)
            .filter(|one| !one.is_empty())
            .map(str::to_string)
            .collect();
        found.push((group, list));
    }
    found
}

/// Put one list back into an override file, leaving every other line of it
/// exactly as it was.
///
/// Rewritten by hand rather than round-tripped through a keyfile writer, so
/// that a comment, an unknown section or a key this store knows nothing about
/// survives being edited here. A store that quietly dropped somebody's
/// `[Environment]` block would be a store nobody trusts twice.
fn rewrite(existing: &str, group: Group, list: &[String]) -> String {
    let line = if list.is_empty() {
        None
    } else {
        Some(format!("{}={};", group.key(), list.join(";")))
    };

    let mut out: Vec<String> = Vec::new();
    let mut in_context = false;
    let mut written = false;
    let mut context_at = None;

    for raw in existing.lines() {
        let trimmed = raw.trim();
        if trimmed.starts_with('[') {
            if in_context && !written {
                if let Some(line) = &line {
                    out.push(line.clone());
                    written = true;
                }
            }
            in_context = trimmed == "[Context]";
            if in_context {
                context_at = Some(out.len());
            }
            out.push(raw.to_string());
            continue;
        }
        if in_context {
            if let Some((key, _)) = trimmed.split_once('=') {
                if key.trim() == group.key() {
                    if let Some(line) = &line {
                        out.push(line.clone());
                    }
                    written = true;
                    continue;
                }
            }
        }
        out.push(raw.to_string());
    }

    if !written {
        if let Some(line) = line {
            match context_at {
                Some(at) => out.insert(at + 1, line),
                None => {
                    if !out.is_empty() && !out.last().is_some_and(|last| last.trim().is_empty()) {
                        out.push(String::new());
                    }
                    out.push("[Context]".into());
                    out.push(line);
                }
            }
        }
    }

    let mut text = out.join("\n");
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const METADATA: &str = "\
[Application]
name=org.example.Player
runtime=org.example.Platform/x86_64/25.08
command=player

[Context]
shared=network;ipc;
sockets=x11;wayland;fallback-x11;pulseaudio;
devices=dri;
filesystems=xdg-run/gvfsd;home;

[Session Bus Policy]
org.freedesktop.secrets=talk
";

    fn toggle(key: &str) -> &'static Toggle {
        TOGGLES
            .iter()
            .find(|one| one.key == key)
            .unwrap_or_else(|| panic!("no switch for {key}"))
    }

    fn sandbox_with(override_text: &str) -> Sandbox {
        Sandbox {
            asked: lists_in(METADATA),
            said: lists_in(override_text),
        }
    }

    #[test]
    fn what_an_application_asked_for_is_what_it_has() {
        let sandbox = sandbox_with("");
        assert_eq!(sandbox.standing(toggle("network")), Standing::Asked);
        assert_eq!(sandbox.standing(toggle("dri")), Standing::Asked);
        assert_eq!(sandbox.standing(toggle("home")), Standing::Asked);
        assert_eq!(
            sandbox.standing(toggle("host")),
            Standing::NotAsked,
            "an application was given the whole machine without asking"
        );
        assert!(
            !sandbox.touched(),
            "an application nobody has edited was shown as edited"
        );
    }

    #[test]
    fn an_override_is_what_wins() {
        let sandbox = sandbox_with("[Context]\nshared=!network;\nfilesystems=host;\n");
        assert_eq!(
            sandbox.standing(toggle("network")),
            Standing::TurnedOff,
            "a permission taken away was still granted"
        );
        assert!(!sandbox.standing(toggle("network")).on());
        assert_eq!(
            sandbox.standing(toggle("host")),
            Standing::TurnedOn,
            "a permission granted here was not granted"
        );
        assert!(sandbox.standing(toggle("host")).on());
        assert!(sandbox.touched(), "an edited sandbox said it was untouched");
    }

    #[test]
    fn an_override_repeating_what_was_asked_for_is_not_a_change() {
        let sandbox = sandbox_with("[Context]\nshared=network;\n");
        assert_eq!(
            sandbox.standing(toggle("network")),
            Standing::Asked,
            "flatpak repeating a permission in an override read as somebody changing it"
        );
        assert!(!sandbox.touched());
    }

    #[test]
    fn a_mode_on_a_filesystem_does_not_hide_it() {
        let sandbox = sandbox_with("[Context]\nfilesystems=!home;xdg-download:ro;\n");
        assert_eq!(
            sandbox.standing(toggle("home")),
            Standing::TurnedOff,
            "`!home` did not take away `home`"
        );
        assert_eq!(
            sandbox.standing(toggle("xdg-download")),
            Standing::TurnedOn,
            "`xdg-download:ro` was not read as Downloads being granted"
        );
    }

    #[test]
    fn everything_asked_for_that_has_no_switch_is_still_said() {
        let sandbox = sandbox_with("");
        let rest = sandbox.also_asked();
        assert!(
            rest.contains(&"xdg-run/gvfsd".to_string()),
            "a permission with no switch of its own vanished: {rest:?}"
        );
        assert!(
            !rest.contains(&"network".to_string()),
            "a permission that has a switch was listed twice: {rest:?}"
        );
    }

    #[test]
    fn turning_something_back_to_what_was_asked_for_removes_the_entry() {
        let sandbox = sandbox_with("[Context]\nshared=!network;\n");
        let written = rewrite(
            "[Context]\nshared=!network;\n",
            Group::Share,
            &[] as &[String],
        );
        assert!(
            !written.contains("shared"),
            "an entry that no longer says anything was left behind: {written}"
        );
        assert!(
            sandbox.standing(toggle("network")).changed(),
            "the fixture was not what this test needs"
        );
    }

    #[test]
    fn everything_else_in_an_override_file_survives_being_edited() {
        let existing = "\
[Context]
shared=!network;
filesystems=host;

[Environment]
LANG=C

[Session Bus Policy]
org.freedesktop.Flatpak=talk
";
        let written = rewrite(existing, Group::Share, &["!network".into(), "ipc".into()]);
        assert!(
            written.contains("LANG=C"),
            "an unrelated section was thrown away: {written}"
        );
        assert!(
            written.contains("org.freedesktop.Flatpak=talk"),
            "a bus policy was thrown away: {written}"
        );
        assert!(
            written.contains("filesystems=host;"),
            "another key in the same section was thrown away: {written}"
        );
        assert!(
            written.contains("shared=!network;ipc;"),
            "the key being written did not come out right: {written}"
        );
        assert_eq!(
            written.matches("[Context]").count(),
            1,
            "a second [Context] was added beside the first: {written}"
        );
    }

    #[test]
    fn a_file_with_no_context_at_all_gets_one() {
        let written = rewrite("", Group::Socket, &["!x11".into()]);
        assert_eq!(written, "[Context]\nsockets=!x11;\n", "got: {written:?}");

        let written = rewrite("[Environment]\nLANG=C\n", Group::Socket, &["!x11".into()]);
        assert!(
            written.contains("LANG=C") && written.contains("sockets=!x11;"),
            "a file with other sections lost one when [Context] was added: {written}"
        );
    }

    #[test]
    fn an_override_is_written_where_flatpak_looks_for_it() {
        let path = overrides_for("org.example.Player");
        assert!(
            path.ends_with("flatpak/overrides/org.example.Player"),
            "overrides would be written somewhere flatpak never reads: {}",
            path.display()
        );
        assert!(path.is_absolute(), "a relative override path: {path:?}");
    }

    /// The whole path, against the real disk: write an override, read it back
    /// through the same reader the page uses, and take it away again.
    ///
    /// Written into a scratch `XDG_DATA_HOME`, so it touches nothing flatpak
    /// will ever read on this machine. Deliberately outside the ordinary suite
    /// because it sets an environment variable, which every other test in this
    /// process would see. Run it by hand:
    /// `cargo test -- --ignored --test-threads 1 an_override_really`.
    #[test]
    #[ignore]
    fn an_override_really_lands_where_it_is_read_back_from() {
        let at = std::env::temp_dir().join(format!("distribumpy-overrides-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&at);
        std::env::set_var("XDG_DATA_HOME", &at);

        let id = "org.example.Player";
        let path = overrides_for(id);
        assert!(
            path.starts_with(&at),
            "the scratch home was not honoured: {}",
            path.display()
        );

        let sandbox = Sandbox::read(METADATA, id);
        assert_eq!(sandbox.standing(toggle("network")), Standing::Asked);

        set(&sandbox, id, toggle("network"), false).expect("an override to be written");
        let written = std::fs::read_to_string(&path).expect("the override file");
        assert!(
            written.contains("shared=!network;"),
            "what was written is not what flatpak reads: {written}"
        );

        let sandbox = Sandbox::read(METADATA, id);
        assert_eq!(
            sandbox.standing(toggle("network")),
            Standing::TurnedOff,
            "an override was written and then not read back"
        );
        assert!(sandbox.touched());

        // And back again: setting it to what was asked for takes the entry out
        // rather than writing its opposite.
        set(&sandbox, id, toggle("network"), true).expect("the override to be undone");
        let sandbox = Sandbox::read(METADATA, id);
        assert_eq!(sandbox.standing(toggle("network")), Standing::Asked);
        assert!(!sandbox.touched(), "an undone change was still a change");

        forget(id).expect("the override file to go");
        assert!(!path.exists(), "the override file was left behind");
        forget(id).expect("forgetting nothing is not a failure");

        let _ = std::fs::remove_dir_all(&at);
        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn every_switch_belongs_to_a_group_that_is_written_somewhere() {
        for toggle in TOGGLES {
            assert!(
                Group::ALL.contains(&toggle.group),
                "{} is in a group nothing writes",
                toggle.title
            );
            assert!(
                !toggle.note.is_empty(),
                "{} says nothing about what it does",
                toggle.title
            );
            assert!(
                !toggle.key.starts_with('!'),
                "{} is named as its own negation",
                toggle.title
            );
        }
    }
}
