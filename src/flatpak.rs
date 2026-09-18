//! Flatpak itself: what is installed, what a remote offers, and the worker
//! that installs, updates and removes things.
//!
//! Everything here goes through `libflatpak`, which is the same library the
//! `flatpak` command is a front end for. That matters for one reason above the
//! others: a `Transaction` resolves dependencies, reports progress operation by
//! operation, and can be cancelled — none of which can be read reliably out of
//! another program's console output.
//!
//! **libflatpak's objects are GObjects and are not `Send`.** Nothing here hands
//! one to another thread. The worker builds its own `Installation` inside the
//! thread that uses it, and what crosses the channel is a plain description of
//! a job and a plain report of how it went.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

use libflatpak::gio::Cancellable;
use libflatpak::prelude::*;

/// Which of the machine's two installations something lives in.
///
/// The user installation is the one nothing has to authorise, and is where
/// this store puts anything it installs. The system installation is where
/// most of what is already on a machine lives, so it is listed, updated and
/// removed as readily — those simply raise the desktop's password panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    User,
    System,
}

impl Scope {
    pub fn title(self) -> &'static str {
        match self {
            Scope::User => crate::i18n::text("this-user"),
            Scope::System => crate::i18n::text("this-system"),
        }
    }

    /// Whether acting in this scope goes through `flatpak-system-helper` at
    /// all, which is the only place authorization can ever be wanted. Whether
    /// it *will* be wanted is [`will_ask`], and is not this question.
    pub fn goes_through_the_helper(self) -> bool {
        self == Scope::System
    }

    fn open(self) -> Result<libflatpak::Installation, glib::Error> {
        match self {
            Scope::User => libflatpak::Installation::new_user(Cancellable::NONE),
            Scope::System => libflatpak::Installation::new_system(Cancellable::NONE),
        }
    }
}

/// A repository, and where its catalogue is kept.
///
/// Every remote of every installation is kept, disabled ones included: a page
/// that lists repositories has to show the one somebody switched off, or there
/// is no way back to it.
#[derive(Debug, Clone)]
pub struct Remote {
    pub name: String,
    pub title: String,
    pub url: String,
    pub scope: Scope,
    pub appstream: PathBuf,
    /// What it says about itself, where it says anything.
    pub description: String,
    pub homepage: String,
    /// Switched off here. It stays configured, and nothing from it is offered.
    pub disabled: bool,
    /// A remote that asked not to be listed. These are the single-application
    /// origins flatpak writes when something is installed from a bundle, and
    /// there is normally one per such application.
    pub noenumerate: bool,
    /// Whether flatpak checks the repository's signature, which is the one
    /// thing about a repository worth saying out loud when it is not true.
    pub gpg_verify: bool,
    pub priority: i32,
}

impl Remote {
    /// Whether this is a repository somebody would want to see on a page.
    ///
    /// The origins written for bundle installs are not: a machine that has
    /// installed six applications from files has six of them, each offering
    /// exactly one thing, and none of them is a repository anybody chose.
    pub fn worth_listing(&self) -> bool {
        !self.noenumerate
    }

    /// Whether anything on offer here should be browsed.
    pub fn worth_reading(&self) -> bool {
        self.worth_listing() && self.worth_installing_from()
    }

    /// Whether anything can actually be fetched from here.
    ///
    /// A switched-off remote stays configured — that is the whole point of
    /// switching one off rather than forgetting it — but flatpak will not
    /// resolve a ref against it, and asking it to says "No such ref". Being
    /// unlisted is deliberately not part of this: a bundle's origin is hidden
    /// from every page and is still where its one application comes from.
    pub fn worth_installing_from(&self) -> bool {
        !self.disabled
    }

    /// What to show as its name, which is not always what flatpak files it
    /// under. A remote added by hand often has no title at all.
    pub fn shown(&self) -> &str {
        if self.title.is_empty() {
            &self.name
        } else {
            &self.title
        }
    }
}

/// One application already on the disk.
#[derive(Debug, Clone)]
pub struct Installed {
    pub id: String,
    pub name: String,
    pub version: String,
    pub branch: String,
    pub origin: String,
    pub size: u64,
    pub scope: Scope,
    pub reference: String,
    /// Set when the remote is offering a newer commit than the one installed.
    pub updatable: bool,
    /// `org.gnome.Platform/x86_64/49` — what it is running on.
    pub runtime: String,
    /// Set where the publisher has said this will not be updated again, with
    /// whatever they said about it. This is the one fact about an installed
    /// application that a store must not bury.
    pub eol: Option<String>,
    /// The application's own metadata, which is what it asked the sandbox for.
    /// Read here because it comes out of the installation, and read once
    /// because a page asks for it on every frame. Empty for everything that
    /// is not an application: nothing asks a runtime what it may reach.
    pub metadata: String,
    /// Whether this is an application, or one of the runtimes, SDKs and
    /// extensions applications stand on.
    ///
    /// **A list of applications must leave those out and a list of updates
    /// must not.** They were not read at all, so an update of everything
    /// fetched eleven things while the shelf said seven and named none of the
    /// other four — Discover calls them Application Support and shows them,
    /// which is how this was noticed.
    pub is_app: bool,
}

/// Everything on the disk, and everywhere worth looking for more.
#[derive(Debug, Default)]
pub struct Machine {
    pub remotes: Vec<Remote>,
    pub installed: Vec<Installed>,
    /// What went wrong while reading, if anything did. A machine with no
    /// flatpak at all is a state to say out loud, not a crash.
    pub trouble: Option<String>,
    /// How much of the user installation nothing needs any more, and how many
    /// refs that is. Runtimes left behind by an application that has since
    /// been removed are where a machine's flatpak disk usage actually goes,
    /// and nothing else on a store's pages would ever mention them.
    pub unused: u64,
    pub unused_count: usize,
}

impl Machine {
    /// Read both installations.
    ///
    /// The user installation comes first throughout, so that where the same
    /// application is offered by both, the one that needs no password wins.
    pub fn read() -> Self {
        // Asked here, on the reader's own thread, because asking it costs a
        // process and the frame loop may not spend one. Every later ask is the
        // answer already in hand. See [`will_ask`].
        for act in [Act::Install, Act::Update, Act::Remove] {
            let _ = will_ask(act);
        }
        let mut machine = Self::default();
        let mut opened = 0;
        for scope in [Scope::User, Scope::System] {
            match scope.open() {
                Ok(installation) => {
                    opened += 1;
                    machine.read_installation(&installation, scope);
                }
                Err(err) => {
                    eprintln!("distribumpy: no {scope:?} installation: {err}");
                }
            }
        }
        if opened == 0 {
            machine.trouble = Some(crate::i18n::text("no-flatpak").into());
        }
        machine.read_what_is_left_over();
        machine
    }

    /// What nothing needs any more, in the installation this store writes to.
    ///
    /// Only the user installation: it is the one a press here can act on
    /// without a password, and offering to clear out the system one and then
    /// asking for a password is a worse answer than not offering.
    fn read_what_is_left_over(&mut self) {
        let Ok(installation) = Scope::User.open() else {
            return;
        };
        let Ok(unused) = installation.list_unused_refs(None, Cancellable::NONE) else {
            return;
        };
        self.unused_count = unused.len();
        self.unused = unused.iter().map(|one| one.installed_size()).sum();
    }

    pub fn installed_app(&self, id: &str) -> Option<&Installed> {
        self.installed.iter().find(|one| one.is_app && one.id == id)
    }

    /// Everything installed that is an application, which is what a list of
    /// applications is. See [`Installed::is_app`].
    pub fn apps(&self) -> impl Iterator<Item = &Installed> {
        self.installed.iter().filter(|one| one.is_app)
    }

    /// One of the runtimes, SDKs or extensions, by what it is and where.
    pub fn support(&self, id: &str, scope: Scope) -> Option<&Installed> {
        self.installed
            .iter()
            .find(|one| !one.is_app && one.id == id && one.scope == scope)
    }

    /// The remotes whose catalogue is worth reading, which is not all of them.
    ///
    /// Flathub is normally configured in both installations, and both point at
    /// the same repository. Reading its 47 MB catalogue twice to throw the
    /// second away costs a second and a half of a cold start and answers
    /// nothing, so one remote per repository is read — the user's, where there
    /// is a choice, because that is the one installing needs no password for.
    pub fn catalogue_remotes(&self) -> Vec<&Remote> {
        let mut seen = std::collections::HashSet::new();
        self.remotes
            .iter()
            .filter(|remote| remote.worth_reading())
            .filter(|remote| seen.insert((remote.name.clone(), remote.url.clone())))
            .collect()
    }

    /// The repositories a page lists, which is every one somebody configured
    /// on purpose — switched off as readily as switched on.
    pub fn listed_remotes(&self) -> Vec<&Remote> {
        self.remotes
            .iter()
            .filter(|remote| remote.worth_listing())
            .collect()
    }

    pub fn remote(&self, name: &str, scope: Scope) -> Option<&Remote> {
        self.remotes
            .iter()
            .find(|one| one.name == name && one.scope == scope)
    }

    /// How many installed applications came from a repository. Applications:
    /// a repository's page is about what somebody chose to install from it,
    /// and every runtime those applications dragged in with them would be a
    /// number four times too big.
    ///
    /// Counted within one installation, because a name is not a remote: with
    /// flathub configured both system-wide and for this user, counting by name
    /// alone credits each of the two rows with the other's applications.
    pub fn installed_from(&self, remote: &str, scope: Scope) -> usize {
        self.apps()
            .filter(|one| one.origin == remote && one.scope == scope)
            .count()
    }

    /// Everything installed that will not be updated again.
    pub fn ending(&self) -> Vec<&Installed> {
        self.installed
            .iter()
            .filter(|one| one.is_app)
            .filter(|one| one.eol.is_some())
            .collect()
    }

    /// Where something offered by this remote should be installed.
    ///
    /// The user installation, whenever it has the remote — nothing has to be
    /// authorised and nothing else on the machine is touched. Where only the
    /// system installation knows the remote, that is the only answer there is,
    /// and the page says so before the press.
    ///
    /// **A name is not a remote.** The same name can be configured in both
    /// installations, and a machine with flathub added system-wide and again
    /// for this user has two of them — the second frequently switched off,
    /// because two copies of Flathub is one more than anybody wants. Asking
    /// only whether the *name* is known here sent every install to a remote
    /// that will not answer, and flatpak said so: "No such ref
    /// 'app/…/x86_64/stable' in remote flathub". The catalogue was being read
    /// from the other one all along, which is why the application was on the
    /// page at all and why switching this one off appeared to do nothing.
    pub fn install_scope(&self, remote: &str) -> Scope {
        if self.remotes.iter().any(|one| {
            one.name == remote && one.scope == Scope::User && one.worth_installing_from()
        }) {
            Scope::User
        } else {
            Scope::System
        }
    }

    /// What a remote calls itself, which is what a person should be shown.
    ///
    /// Falls back to the name flatpak files it under, because a remote added
    /// by hand often has no title at all.
    pub fn remote_title(&self, name: &str) -> String {
        self.remotes
            .iter()
            .find(|one| one.name == name)
            .map(|one| one.title.clone())
            .unwrap_or_else(|| name.to_string())
    }

    pub fn updatable(&self) -> Vec<&Installed> {
        self.installed.iter().filter(|one| one.updatable).collect()
    }

    fn read_installation(&mut self, installation: &libflatpak::Installation, scope: Scope) {
        let stale: std::collections::HashSet<String> = installation
            .list_installed_refs_for_update(Cancellable::NONE)
            .map(|refs| {
                refs.iter()
                    .filter_map(|one| one.format_ref().map(|text| text.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        for entry in installation
            .list_installed_refs(Cancellable::NONE)
            .unwrap_or_default()
        {
            let is_app = entry.kind() == libflatpak::RefKind::App;
            let Some(id) = entry.name().map(|name| name.to_string()) else {
                continue;
            };
            let reference = entry
                .format_ref()
                .map(|text| text.to_string())
                .unwrap_or_default();
            // Read once, here, because a detail page asks what an application
            // may reach on every frame and this comes off the disk. Only for
            // applications: a machine has a couple of hundred runtimes and
            // extensions, none of them has a permissions page, and reading
            // every one of them off the disk would be two hundred reads spent
            // on nothing.
            let metadata = if is_app {
                entry
                    .load_metadata(Cancellable::NONE)
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                    .unwrap_or_default()
            } else {
                String::new()
            };

            self.installed.push(Installed {
                runtime: value_in(&metadata, "runtime"),
                eol: entry
                    .eol()
                    .map(|said| said.to_string())
                    .or_else(|| {
                        entry
                            .eol_rebase()
                            .map(|to| crate::message!("renamed-to", "to" => (to).to_string()))
                    })
                    .map(|said| {
                        if said.trim().is_empty() {
                            crate::i18n::text("publisher-stopped-updating").to_string()
                        } else {
                            said
                        }
                    }),
                metadata,
                name: entry
                    .appdata_name()
                    .map(|name| name.to_string())
                    .unwrap_or_else(|| id.clone()),
                version: entry
                    .appdata_version()
                    .map(|version| version.to_string())
                    .unwrap_or_default(),
                branch: entry
                    .branch()
                    .map(|branch| branch.to_string())
                    .unwrap_or_default(),
                origin: entry
                    .origin()
                    .map(|origin| origin.to_string())
                    .unwrap_or_default(),
                size: entry.installed_size(),
                is_app,
                scope,
                updatable: stale.contains(&reference),
                reference,
                id,
            });
        }

        for remote in installation
            .list_remotes(Cancellable::NONE)
            .unwrap_or_default()
        {
            let Some(name) = remote.name().map(|name| name.to_string()) else {
                continue;
            };
            let appstream = remote
                .appstream_dir(Some(&arch()))
                .and_then(|directory| directory.path())
                .unwrap_or_default();
            self.remotes.push(Remote {
                title: remote
                    .title()
                    .map(|title| title.to_string())
                    .unwrap_or_default(),
                url: remote.url().map(|url| url.to_string()).unwrap_or_default(),
                description: remote
                    .description()
                    .or_else(|| remote.comment())
                    .map(|said| said.to_string())
                    .unwrap_or_default(),
                homepage: remote
                    .homepage()
                    .map(|url| url.to_string())
                    .unwrap_or_default(),
                disabled: remote.is_disabled(),
                noenumerate: remote.is_noenumerate(),
                gpg_verify: remote.is_gpg_verify(),
                priority: remote.prio(),
                name,
                scope,
                appstream,
            });
        }
    }
}

/// One value out of a flatpak metadata file, by key.
///
/// The file is a keyfile and this reads one key out of it without caring which
/// section it is in, which is enough for `runtime` — the only key here that is
/// wanted outside `[Context]`, and one that appears exactly once.
fn value_in(metadata: &str, key: &str) -> String {
    metadata
        .lines()
        .filter_map(|line| line.trim().split_once('='))
        .find(|(name, _)| name.trim() == key)
        .map(|(_, value)| value.trim().to_string())
        .unwrap_or_default()
}

/// The architecture flatpak built this machine's refs for, which is not always
/// the one this binary was compiled for.
pub fn arch() -> String {
    libflatpak::functions::default_arch()
        .map(|arch| arch.to_string())
        .unwrap_or_else(|| "x86_64".into())
}

/// What a repositories page asks the worker to do.
///
/// Every one of these writes to an installation's configuration, so every one
/// of them goes through the same worker the transactions do — two threads
/// writing to one installation is the one thing flatpak will not forgive.
#[derive(Debug, Clone)]
pub enum RepoJob {
    /// Switch a repository off without forgetting it, or back on again.
    Enable { name: String, on: bool },
    /// Forget a repository entirely. Anything installed from it stays
    /// installed, and stops being offered updates.
    Forget { name: String },
    /// Add a repository from the address of a `.flatpakrepo` file, which is
    /// how every repository worth adding is published.
    Add { name: String, url: String },
    /// Fetch a repository's catalogue again, which is what makes something
    /// published in the last hour appear on a shelf.
    Refresh { name: String },
}

impl RepoJob {
    pub fn name(&self) -> &str {
        match self {
            RepoJob::Enable { name, .. }
            | RepoJob::Forget { name }
            | RepoJob::Add { name, .. }
            | RepoJob::Refresh { name } => name,
        }
    }

    fn doing(&self) -> String {
        match self {
            RepoJob::Enable { name, on: true } => {
                crate::message!("switching-on", "name" => (name).to_string())
            }
            RepoJob::Enable { name, on: false } => {
                crate::message!("switching-off", "name" => (name).to_string())
            }
            RepoJob::Forget { name } => {
                crate::message!("forgetting-remote", "name" => (name).to_string())
            }
            RepoJob::Add { name, .. } => {
                crate::message!("adding-remote", "name" => (name).to_string())
            }
            RepoJob::Refresh { name } => {
                crate::message!("fetching-catalogue", "name" => (name).to_string())
            }
        }
    }
}

/// What the page asks the worker to do.
#[derive(Debug, Clone)]
pub enum Job {
    Install {
        scope: Scope,
        remote: String,
        reference: String,
    },
    Update {
        scope: Scope,
        reference: String,
    },
    Remove {
        scope: Scope,
        reference: String,
    },
    /// Update everything in one installation that has an update waiting.
    UpdateAll {
        scope: Scope,
    },
    /// Take away the runtimes nothing installed needs any more, which is where
    /// a machine's flatpak disk usage actually goes.
    Trim {
        scope: Scope,
    },
    /// What an install would fetch, worked out without fetching any of it.
    ///
    /// The transaction is resolved and then refused at the last moment before
    /// the first byte, which is the only way to learn the real number: it
    /// includes every runtime and extension the application needs and does not
    /// already have.
    Weigh {
        scope: Scope,
        remote: String,
        reference: String,
    },
    /// Start an installed application, and get out of the way.
    Open {
        scope: Scope,
        id: String,
    },
    Repository {
        scope: Scope,
        job: RepoJob,
    },
}

impl Job {
    /// What this job is about, which is what an ending is matched against.
    pub fn about(&self) -> String {
        match self {
            Job::Install { reference, .. }
            | Job::Update { reference, .. }
            | Job::Remove { reference, .. }
            | Job::Weigh { reference, .. } => reference.clone(),
            Job::Open { id, .. } => id.clone(),
            Job::UpdateAll { scope } => format!("update-all/{scope:?}"),
            Job::Trim { scope } => format!("trim/{scope:?}"),
            Job::Repository { scope, job } => format!("repo/{scope:?}/{}", job.name()),
        }
    }

    /// Whether this job mutates exactly this application.
    ///
    /// Transaction jobs carry a full Flatpak reference while an open job
    /// carries the application ID directly. Compare the reference component,
    /// not a substring: `org.example.App` and `org.example.App.Beta` are two
    /// different destinations.
    pub fn belongs_to_app(&self, id: &str) -> bool {
        match self {
            Job::Install { reference, .. }
            | Job::Update { reference, .. }
            | Job::Remove { reference, .. }
            | Job::Weigh { reference, .. } => reference.split('/').nth(1) == Some(id),
            Job::Open { id: running, .. } => running == id,
            Job::UpdateAll { .. } | Job::Trim { .. } | Job::Repository { .. } => false,
        }
    }

    /// Whether this job belongs to exactly this repository page.
    pub fn belongs_to_repository(&self, name: &str, scope: Scope) -> bool {
        matches!(
            self,
            Job::Repository {
                scope: running_scope,
                job,
            } if *running_scope == scope && job.name() == name
        )
    }

    pub fn scope(&self) -> Scope {
        match self {
            Job::Install { scope, .. }
            | Job::Update { scope, .. }
            | Job::Remove { scope, .. }
            | Job::UpdateAll { scope }
            | Job::Trim { scope }
            | Job::Weigh { scope, .. }
            | Job::Open { scope, .. }
            | Job::Repository { scope, .. } => *scope,
        }
    }

    /// Whether this is worth showing a progress bar for.
    ///
    /// Weighing and opening are over before anybody could read a bar, and a
    /// bar that flashed for a tenth of a second would only look like a fault.
    pub fn shown(&self) -> bool {
        !matches!(self, Job::Weigh { .. } | Job::Open { .. })
    }

    /// What the page says is happening, in the present tense.
    pub fn doing(&self) -> String {
        match self {
            Job::Install { .. } => crate::i18n::text("installing").into(),
            Job::Update { .. } => crate::i18n::text("updating").into(),
            Job::Remove { .. } => crate::i18n::text("removing").into(),
            Job::UpdateAll { .. } => crate::i18n::text("updating-everything").into(),
            Job::Trim { .. } => crate::i18n::text("clearing-out-what-nothing-needs").into(),
            Job::Weigh { .. } => crate::i18n::text("working-out-the-download").into(),
            Job::Open { .. } => crate::i18n::text("opening").into(),
            Job::Repository { job, .. } => job.doing(),
        }
    }
}

/// What the worker says back.
///
/// `Step` is one operation of a transaction — a runtime, an extension, the
/// application itself — because a transaction that installs one application
/// routinely runs a dozen, and a bar that restarted without saying why would
/// look like a fault.
#[derive(Debug, Clone)]
pub enum Report {
    Step {
        what: String,
        at: usize,
        of: usize,
    },
    Progress {
        /// Nought to one across the whole transaction, not one operation.
        through: f32,
        transferred: u64,
        status: String,
    },
    /// What an install would cost, answered before anything was fetched.
    Weighed {
        reference: String,
        download: u64,
        installed: u64,
    },
    Done {
        about: String,
        /// What to say about it, where an ending is worth a sentence.
        said: String,
    },
    Failed {
        about: String,
        why: String,
    },
}

/// The thread that runs transactions, and the two channels to it.
pub struct Worker {
    jobs: Sender<Job>,
    reports: Receiver<Report>,
    /// The cancellable of whatever is running, so that a press on Stop reaches
    /// it. `GCancellable` is one of the few GObjects that is safe to touch
    /// from another thread, which is the whole reason a cancel is possible.
    stopping: std::sync::Arc<std::sync::Mutex<Option<Cancellable>>>,
}

impl Worker {
    /// Start the worker.
    ///
    /// One thread, and one job at a time on purpose: two transactions against
    /// one installation would contend for the same repository lock, and the
    /// second would fail with something no one could act on.
    pub fn start() -> Self {
        let (jobs, orders) = std::sync::mpsc::channel::<Job>();
        let (voice, reports) = std::sync::mpsc::channel::<Report>();
        let stopping: std::sync::Arc<std::sync::Mutex<Option<Cancellable>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));

        let held = stopping.clone();
        std::thread::Builder::new()
            .name("distribumpy-flatpak".into())
            .spawn(move || {
                while let Ok(job) = orders.recv() {
                    let about = job.about();
                    let stop = Cancellable::new();
                    if let Ok(mut held) = held.lock() {
                        *held = Some(stop.clone());
                    }
                    let answer = run(&job, &voice, &stop);
                    if let Ok(mut held) = held.lock() {
                        *held = None;
                    }
                    let _ = match answer {
                        Ok(said) => voice.send(Report::Done { about, said }),
                        Err(why) => voice.send(Report::Failed { about, why }),
                    };
                }
            })
            .expect("a worker thread");
        Self {
            jobs,
            reports,
            stopping,
        }
    }

    pub fn send(&self, job: Job) {
        let _ = self.jobs.send(job);
    }

    /// Stop whatever is running. What has already been fetched stays fetched,
    /// which is what makes starting again cheap.
    pub fn stop(&self) {
        if let Ok(held) = self.stopping.lock() {
            if let Some(stop) = held.as_ref() {
                stop.cancel();
            }
        }
    }

    /// Everything the worker has said since the last frame.
    pub fn heard(&self) -> Vec<Report> {
        self.reports.try_iter().collect()
    }
}

fn run(job: &Job, voice: &Sender<Report>, stop: &Cancellable) -> Result<String, String> {
    let installation = job.scope().open().map_err(
        |err| crate::message!("installation-cannot-be-opened", "why" => (err).to_string()),
    )?;

    match job {
        Job::Open { id, .. } => {
            // Ask what is installed, and start that.
            //
            // `launch` documents its branch default as "master", which is
            // flatpak's own default and is not what anybody has: everything
            // from Flathub is on `stable`. Passing None therefore asked for a
            // ref that does not exist, and said so —
            // "app/org.audacityteam.Audacity/x86_64/master not installed" —
            // for every application on the machine. The architecture goes the
            // same way rather than being assumed, so an x86_64 build running
            // under an aarch64 installation is started rather than missed.
            let installed = installation
                .current_installed_app(id, Some(stop))
                .map_err(|err| crate::message!("not-installed-here", "why" => (err).to_string()))?;
            let arch = installed.arch();
            let branch = installed.branch();
            return installation
                .launch(id, arch.as_deref(), branch.as_deref(), None, Some(stop))
                .map(|()| String::new())
                .map_err(|err| crate::message!("would-not-start", "why" => (err).to_string()));
        }
        Job::Repository { job, .. } => return repository(&installation, job, stop),
        _ => {}
    }

    let transaction = libflatpak::Transaction::for_installation(&installation, Some(stop))
        .map_err(|err| crate::message!("cannot-be-started", "why" => (err).to_string()))?;

    // Every other installation on the machine counts as somewhere a runtime
    // may already be. Without this, installing a 762 kB application for one
    // user downloads the 759 MB GNOME runtime the system installation is
    // already holding — the transaction will only look inside the one
    // installation it was built for.
    transaction.add_default_dependency_sources();

    match job {
        Job::Install {
            remote, reference, ..
        }
        | Job::Weigh {
            remote, reference, ..
        } => transaction
            .add_install(remote, reference, &[])
            .map_err(|err| crate::message!("cannot-be-installed", "why" => (err).to_string()))?,
        Job::Update { reference, .. } => transaction
            .add_update(reference, &[], None)
            .map_err(|err| crate::message!("cannot-be-updated", "why" => (err).to_string()))?,
        Job::Remove { reference, .. } => transaction
            .add_uninstall(reference)
            .map_err(|err| crate::message!("cannot-be-removed", "why" => (err).to_string()))?,
        Job::UpdateAll { .. } => {
            let stale = installation
                .list_installed_refs_for_update(Some(stop))
                .map_err(
                    |err| crate::message!("out-of-date-cannot-be-read", "why" => (err).to_string()),
                )?;
            if stale.is_empty() {
                return Ok(crate::i18n::text("was-already-up-to-date").into());
            }
            for one in &stale {
                if let Some(reference) = one.format_ref() {
                    // One that will not update is not a reason to update
                    // nothing: an application whose remote is gone is exactly
                    // the case where the rest still need doing.
                    let _ = transaction.add_update(&reference, &[], None);
                }
            }
        }
        Job::Trim { .. } => {
            let unused = installation.list_unused_refs(None, Some(stop)).map_err(
                |err| crate::message!("left-over-cannot-be-worked-out", "why" => (err).to_string()),
            )?;
            if unused.is_empty() {
                return Ok(crate::i18n::text("nothing-was-left-over").into());
            }
            let mut freed = 0;
            for one in &unused {
                if let Some(reference) = one.format_ref() {
                    if transaction.add_uninstall(&reference).is_ok() {
                        freed += one.installed_size();
                    }
                }
            }
            let said = crate::message!("took-back-size", "size" => size(freed));
            let (went, refused) = run_it(&transaction, voice, job, stop)?;
            if !refused.is_empty() {
                return Err(what_would_not_go(went, &refused));
            }
            return Ok(said);
        }
        Job::Open { .. } | Job::Repository { .. } => unreachable!("answered above"),
    }

    if let Job::Weigh { reference, .. } = job {
        return weigh(&transaction, voice, reference, stop);
    }

    let (went, refused) = run_it(&transaction, voice, job, stop)?;
    if !refused.is_empty() {
        // Said as a failure and not as a note, because it is one: something
        // the user asked for did not happen, and a line in the same ink as
        // "Everything was already up to date" is a line nobody reads.
        return Err(what_would_not_go(went, &refused));
    }
    Ok(String::new())
}

/// What to say when a transaction is over and some of it would not go.
///
/// **Naming the first one is the whole point.** "Stopped." was said about a
/// list of eleven updates that had done nine of them, and there was nowhere on
/// the page to find out which two had failed or why — so the only way to get
/// anywhere was to try them one at a time until one of them said something.
///
/// How many did go is the other half. Without it there is no telling a run
/// that did nearly everything from one that did nothing at all, and those two
/// want different things done about them.
fn what_would_not_go(went: usize, refused: &[String]) -> String {
    let first = refused.first().map(String::as_str).unwrap_or_default();
    match (went, refused.len()) {
        (_, 0) => String::new(),
        (0, 1) => first.to_string(),
        (0, more) => {
            crate::message!("more-would-not-go", "more" => more, "first" => first.to_string())
        }
        (went, 1) => {
            crate::message!("one-would-not-go", "went" => went, "first" => first.to_string())
        }
        (went, more) => {
            crate::message!("some-would-not-go", "went" => went, "more" => more, "first" => first.to_string())
        }
    }
}

/// Resolve a transaction and refuse it at `ready`, which is the last moment
/// before anything at all is fetched, and say what it would have cost.
fn weigh(
    transaction: &libflatpak::Transaction,
    voice: &Sender<Report>,
    reference: &str,
    stop: &Cancellable,
) -> Result<String, String> {
    let heard = voice.clone();
    let about = reference.to_string();
    transaction.connect_ready(move |transaction| {
        let mut download = 0;
        let mut installed = 0;
        for operation in transaction.operations() {
            download += operation.download_size();
            installed += operation.installed_size();
        }
        let _ = heard.send(Report::Weighed {
            reference: about.clone(),
            download,
            installed,
        });
        // Refused on purpose. Everything above this line is arithmetic.
        false
    });

    match transaction.run(Some(stop)) {
        Ok(()) => Ok(String::new()),
        // Refusing at `ready` is reported as an abort, which is what was
        // wanted and is not something to put in front of anybody.
        Err(err) if aborted(&err) => Ok(String::new()),
        Err(err) => Err(tidy(&err.to_string())),
    }
}

fn run_it(
    transaction: &libflatpak::Transaction,
    voice: &Sender<Report>,
    job: &Job,
    stop: &Cancellable,
) -> Result<(usize, Vec<String>), String> {
    let total = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let done = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let refused: std::rc::Rc<std::cell::RefCell<Vec<String>>> = Default::default();

    let counted = total.clone();
    transaction.connect_ready(move |transaction| {
        counted.set(transaction.operations().len().max(1));
        true
    });

    let spoken = voice.clone();
    let at = done.clone();
    let of = total.clone();
    let doing = job.doing();
    transaction.connect_new_operation(move |_, operation, progress| {
        let step = at.get() + 1;
        at.set(step);
        let _ = spoken.send(Report::Step {
            what: describe(operation, &doing),
            at: step,
            of: of.get().max(step),
        });

        let heard = spoken.clone();
        let inside_step = step;
        let inside_of = of.clone();
        progress.set_update_frequency(250);
        progress.connect_changed(move |progress| {
            let of = inside_of.get().max(inside_step) as f32;
            let within = (progress.progress().clamp(0, 100) as f32) / 100.0;
            let through = ((inside_step - 1) as f32 + within) / of;
            let _ = heard.send(Report::Progress {
                through: through.clamp(0.0, 1.0),
                transferred: progress.bytes_transferred(),
                status: progress
                    .status()
                    .map(|status| status.to_string())
                    .unwrap_or_default(),
            });
        });
    });

    // **One operation that will not go is not a reason to abandon the other
    // ten.** With nothing listening for this, libflatpak's answer to a failed
    // operation is to give up on the whole transaction and report it as
    // *aborted* — which `tidy` then read as somebody having pressed Stop. An
    // update of everything that met one application whose runtime it could not
    // fetch therefore left every application after it in the list undone, and
    // said "Stopped." about it. So each failure is written down and the rest
    // of the list is carried on with, and what would not go is named at the
    // end. The one thing not carried on past is a cancellation: that *is*
    // somebody pressing Stop, and the rest of the list is precisely what they
    // asked not to have done.
    let written = refused.clone();
    let doing = job.doing();
    transaction.connect_operation_error(move |_, operation, error, _| {
        if error.matches(libflatpak::gio::IOErrorEnum::Cancelled) {
            return false;
        }
        written
            .borrow_mut()
            .push(format!("{} — {}", describe(operation, &doing), error));
        true
    });

    let answer = transaction.run(Some(stop));
    let failures = refused.take();
    // What really went, which is what makes the difference between "nine of
    // these are done and two are not" and "nothing happened".
    let went = done.get().saturating_sub(failures.len());
    match answer {
        Ok(()) => Ok((went, failures)),
        // A run that failed *and* named the operations it failed on is
        // answered by the list: the library's own last word for that is
        // "aborted due to failure", which says nothing anybody can act on.
        Err(_) if !failures.is_empty() => Ok((went, failures)),
        Err(err) => Err(tidy(&err.to_string())),
    }
}

/// Everything that writes to a repository's configuration.
fn repository(
    installation: &libflatpak::Installation,
    job: &RepoJob,
    stop: &Cancellable,
) -> Result<String, String> {
    match job {
        RepoJob::Enable { name, on } => {
            let remote = installation
                .remote_by_name(name, Some(stop))
                .map_err(|err| crate::message!("remote-not-configured", "name" => (name).to_string(), "why" => (err).to_string()))?;
            remote.set_disabled(!on);
            installation
                .modify_remote(&remote, Some(stop))
                .map_err(|err| crate::message!("remote-could-not-be-changed", "name" => (name).to_string(), "why" => (err).to_string()))?;
            Ok(if *on {
                crate::message!("remote-is-on-again", "name" => (name).to_string())
            } else {
                crate::message!("remote-is-off", "name" => (name).to_string())
            })
        }
        RepoJob::Forget { name } => {
            installation
                .remove_remote(name, Some(stop))
                .map_err(|err| {
                    crate::message!("remote-could-not-be-forgotten", "name" => name.to_string(), "why" => tidy(&err.to_string()))
                })?;
            Ok(crate::message!("remote-is-gone", "name" => (name).to_string()))
        }
        RepoJob::Refresh { name } => {
            installation
                .update_appstream_sync(name, Some(&arch()), Some(stop))
                .map_err(|err| crate::message!("catalogue-could-not-be-fetched", "name" => (name).to_string(), "why" => (err).to_string()))?;
            Ok(crate::message!("catalogue-is-up-to-date", "name" => (name).to_string()))
        }
        RepoJob::Add { name, url } => {
            let described = describe_repository(url)?;
            let remote = libflatpak::Remote::from_file(name, &glib::Bytes::from_owned(described))
                .map_err(
                |err| crate::message!("not-a-repository-description", "why" => (err).to_string()),
            )?;
            // `if_needed` is false on purpose: a name already taken should say
            // so rather than quietly leave the old repository in place.
            installation
                .add_remote(&remote, false, Some(stop))
                .map_err(|err| crate::message!("remote-could-not-be-added", "name" => name.to_string(), "why" => tidy(&err.to_string())))?;
            let _ = installation.update_appstream_sync(name, Some(&arch()), Some(stop));
            Ok(crate::message!("remote-is-here", "name" => (name).to_string()))
        }
    }
}

/// Fetch a `.flatpakrepo` file, which is the small text file that describes a
/// repository — its address, its title and the key its commits are signed
/// with.
///
/// Deliberately small and deliberately over https: this file decides what
/// signature flatpak will trust from that repository ever after.
fn describe_repository(url: &str) -> Result<Vec<u8>, String> {
    if !url.starts_with("https://") {
        return Err(crate::i18n::text("repository-needs-https").into());
    }
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .build()
        .new_agent();
    let mut response = agent
        .get(url)
        .call()
        .map_err(|err| crate::message!("url-could-not-be-read", "url" => (url).to_string(), "why" => (err).to_string()))?;
    let described = response
        .body_mut()
        .with_config()
        .limit(64 * 1024)
        .read_to_vec()
        .map_err(|err| crate::message!("url-could-not-be-read", "url" => (url).to_string(), "why" => (err).to_string()))?;
    if described.is_empty() {
        return Err(crate::message!("url-is-empty", "url" => (url).to_string()));
    }
    Ok(described)
}

/// Whether an error is a transaction that was refused rather than one that
/// went wrong.
fn aborted(err: &glib::Error) -> bool {
    let said = err.to_string().to_lowercase();
    said.contains("abort") || said.contains("cancel")
}

fn describe(operation: &libflatpak::TransactionOperation, doing: &str) -> String {
    let name = operation
        .get_ref()
        .map(|text| text.to_string())
        .unwrap_or_default();
    let name = name
        .split('/')
        .nth(1)
        .map(str::to_string)
        .unwrap_or_else(|| name.clone());
    match operation.operation_type() {
        libflatpak::TransactionOperationType::Install
        | libflatpak::TransactionOperationType::InstallBundle => {
            crate::message!("installing-name", "name" => (name).to_string())
        }
        libflatpak::TransactionOperationType::Update => {
            crate::message!("updating-name", "name" => (name).to_string())
        }
        libflatpak::TransactionOperationType::Uninstall => {
            crate::message!("removing-name", "name" => (name).to_string())
        }
        _ => format!("{doing} {name}"),
    }
}

/// What a person can act on, out of what the library said.
///
/// The two worth naming are a refused password and no network, because both
/// are things the user can do something about and neither reads that way in
/// the message flatpak produces.
fn tidy(trouble: &str) -> String {
    let lowered = trouble.to_lowercase();
    if lowered.contains("not authorized") || lowered.contains("dismissed") {
        return crate::i18n::text("permission-not-given").into();
    }
    // **"Stopped." is a thing the user did**, and it may not be said about
    // anything else. libflatpak calls a transaction that gave up "aborted"
    // whichever way it gave up — Stop pressed, or one operation that failed —
    // and folding the two together is what made a half-done update of
    // everything report itself as a cancelled one, with the real reason
    // nowhere on the page. See `run_it`, where a failure is now carried past
    // and named instead.
    if lowered.contains("cancel") || lowered.contains("aborted by user") {
        return crate::i18n::text("stopped-what-was-fetched-stays").into();
    }
    if lowered.contains("aborted due to failure") {
        return crate::i18n::text("some-would-not-go-unnamed").into();
    }
    if lowered.contains("already exists") || lowered.contains("already installed") {
        return crate::message!("name-is-taken", "trouble" => (trouble).to_string());
    }
    if lowered.contains("resolve") || lowered.contains("network") || lowered.contains("connect") {
        return crate::message!("could-not-reach-the-remote", "trouble" => (trouble).to_string());
    }
    trouble.to_string()
}

/// The three things this store does that a system installation may want
/// authorization for, and the polkit action each one really is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Install,
    Update,
    Remove,
}

impl Act {
    fn action(self) -> &'static str {
        match self {
            Act::Install => "org.freedesktop.Flatpak.app-install",
            Act::Update => "org.freedesktop.Flatpak.app-update",
            Act::Remove => "org.freedesktop.Flatpak.app-uninstall",
        }
    }
}

/// Whether this machine will really ask its owner to prove who they are
/// before it will do this to the system installation.
///
/// **Asked, not guessed.** "System" is not the answer, and saying it was is
/// what put *Authorization required* on pages where no panel would ever
/// appear. Flatpak's own policy is that an **update** needs no authorization
/// at all — the commit is signed, and unattended updates would be impossible
/// otherwise — and the polkit rules it ships grant install and uninstall
/// outright to a local user in `wheel`, which on a single-person machine is
/// its owner.
///
/// polkit is the only thing that knows, and `pkcheck` is how it is asked
/// without a D-Bus client of our own. There is no `--allow-user-interaction`,
/// so it never raises a panel and never grants anything: it answers what
/// *would* happen, and nothing else. Asked once for each act and remembered,
/// because it is a fact about the machine rather than about the application
/// being looked at.
///
/// Where polkit cannot be asked at all, nothing is said. A prompt nobody
/// warned about is a smaller fault than a warning about a prompt that cannot
/// appear — and the panel, if it comes, explains itself.
pub fn will_ask(act: Act) -> bool {
    static ANSWERS: [std::sync::OnceLock<bool>; 3] = [
        std::sync::OnceLock::new(),
        std::sync::OnceLock::new(),
        std::sync::OnceLock::new(),
    ];
    let held = match act {
        Act::Install => &ANSWERS[0],
        Act::Update => &ANSWERS[1],
        Act::Remove => &ANSWERS[2],
    };
    *held.get_or_init(|| {
        std::process::Command::new("pkcheck")
            .arg("--action-id")
            .arg(act.action())
            .arg("--process")
            .arg(std::process::id().to_string())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|answer| !answer.success())
            .unwrap_or(false)
    })
}

/// A repository somebody might want and cannot be expected to type out.
///
/// Four, not forty: this is the list of repositories that are worth offering
/// to a person who has just installed a machine, and every one of them is
/// published by the project whose name is on it. Anything else is added by its
/// address, which is the route that has no list to keep up to date.
pub struct Known {
    pub name: &'static str,
    pub title: &'static str,
    /// Catalog message ID for the repository description.
    pub note: &'static str,
    pub url: &'static str,
}

pub const KNOWN: &[Known] = &[
    Known {
        name: "flathub",
        title: "Flathub",
        note: "repository-flathub-note",
        url: "https://dl.flathub.org/repo/flathub.flatpakrepo",
    },
    Known {
        name: "flathub-beta",
        title: "Flathub beta",
        note: "repository-flathub-beta-note",
        url: "https://dl.flathub.org/beta-repo/flathub-beta.flatpakrepo",
    },
    Known {
        name: "gnome-nightly",
        title: "GNOME Nightly",
        note: "repository-gnome-nightly-note",
        url: "https://nightly.gnome.org/gnome-nightly.flatpakrepo",
    },
    Known {
        name: "kdeapps",
        title: "KDE Nightly",
        note: "repository-kdeapps-note",
        url: "https://distribute.kde.org/kdeapps.flatpakrepo",
    },
];

/// A size in bytes, as a store would say it.
pub fn size(bytes: u64) -> String {
    const STEP: f64 = 1024.0;
    let bytes = bytes as f64;
    if bytes < STEP {
        return format!("{bytes:.0} B");
    }
    for (at, unit) in ["kB", "MB", "GB", "TB"].iter().enumerate() {
        let scaled = bytes / STEP.powi(at as i32 + 1);
        if scaled < STEP || *unit == "TB" {
            return if scaled < 10.0 {
                format!("{scaled:.1} {unit}")
            } else {
                format!("{scaled:.0} {unit}")
            };
        }
    }
    format!("{bytes:.0} B")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A remote with only the parts these tests are about filled in.
    fn remote_named(name: &str, scope: Scope, disabled: bool) -> Remote {
        Remote {
            name: name.to_string(),
            title: String::new(),
            url: String::new(),
            scope,
            appstream: PathBuf::new(),
            description: String::new(),
            homepage: String::new(),
            disabled,
            noenumerate: false,
            gpg_verify: true,
            priority: 1,
        }
    }

    fn machine_with(remotes: Vec<Remote>) -> Machine {
        Machine {
            remotes,
            installed: Vec::new(),
            trouble: None,
            unused: 0,
            unused_count: 0,
        }
    }

    #[test]
    fn a_switched_off_remote_is_not_where_anything_is_installed() {
        // The shape of a real machine: Flathub added system-wide, added again
        // for this user, and the user's copy switched off because two of them
        // is one more than anybody wants. The catalogue comes from the system
        // one — the user's is not read at all — so an install has to go there
        // too. Sending it to the user installation asks a remote that will not
        // answer, and flatpak says "No such ref … in remote flathub".
        let machine = machine_with(vec![
            remote_named("flathub", Scope::System, false),
            remote_named("flathub", Scope::User, true),
        ]);
        assert_eq!(
            machine.install_scope("flathub"),
            Scope::System,
            "an install was sent to a remote that is switched off"
        );

        // Switched back on, the user installation is the right answer again:
        // nothing has to be authorised and nothing else on the machine moves.
        let machine = machine_with(vec![
            remote_named("flathub", Scope::System, false),
            remote_named("flathub", Scope::User, false),
        ]);
        assert_eq!(
            machine.install_scope("flathub"),
            Scope::User,
            "an install that needed no password was sent to the whole system"
        );
    }

    #[test]
    fn a_switched_off_remote_is_still_listed_but_never_read() {
        let off = remote_named("flathub", Scope::User, true);
        assert!(
            off.worth_listing(),
            "a repository somebody switched off vanished from the page that switches it back on"
        );
        assert!(
            !off.worth_reading(),
            "a switched-off repository was browsed"
        );
        assert!(
            !off.worth_installing_from(),
            "a switched-off repository was treated as a source"
        );
    }

    /// The triple a launch is built from, against the real machine.
    ///
    /// Nothing is started: this only asks whether the ref that `Job::Open`
    /// would name is one this installation actually has. It is the check that
    /// would have caught "app/…/x86_64/master not installed" — every
    /// application on a normal machine is on `stable`, and `launch` with no
    /// branch asks for `master`. Run it by hand:
    /// `cargo test -- --ignored --nocapture the_ref_a_launch_names`
    #[test]
    #[ignore]
    fn the_ref_a_launch_names_is_one_that_is_installed() {
        let machine = Machine::read();
        let mut checked = 0;
        for app in machine.apps() {
            let Ok(installation) = app.scope.open() else {
                continue;
            };
            let installed = installation
                .current_installed_app(&app.id, Cancellable::NONE)
                .unwrap_or_else(|err| panic!("{} could not be looked up: {err}", app.id));
            let arch = installed.arch().unwrap_or_default();
            let branch = installed.branch().unwrap_or_default();
            println!("{} -> app/{}/{arch}/{branch}", app.id, app.id);
            assert!(
                !branch.is_empty(),
                "{}: a launch would fall back to flatpak's default branch, which is master",
                app.id
            );
            assert!(
                installation
                    .installed_ref(
                        libflatpak::RefKind::App,
                        &app.id,
                        Some(&arch),
                        Some(&branch),
                        Cancellable::NONE,
                    )
                    .is_ok(),
                "{}: the ref a launch would name is not installed here",
                app.id
            );
            checked += 1;
        }
        println!("{checked} applications checked");
    }

    #[test]
    fn a_size_is_said_the_way_a_store_says_it() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(999), "999 B");
        assert_eq!(size(1024), "1.0 kB");
        assert_eq!(size(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(size(700 * 1024 * 1024), "700 MB");
        assert_eq!(size(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn every_repository_worth_offering_is_described_over_https() {
        for known in KNOWN {
            assert!(
                known.url.starts_with("https://"),
                "{} would be fetched in the clear, and its signing key with it",
                known.title
            );
            assert!(
                known.url.ends_with(".flatpakrepo"),
                "{} is not a repository description: {}",
                known.title,
                known.url
            );
            assert!(!known.name.is_empty() && !known.note.is_empty());
        }
    }

    #[test]
    fn a_repository_is_only_described_over_https() {
        assert!(
            describe_repository("http://example.invalid/one.flatpakrepo").is_err(),
            "a signing key would have been fetched over a connection anybody can rewrite"
        );
        assert!(
            describe_repository("file:///etc/passwd").is_err(),
            "a local file was read as a repository description"
        );
    }

    #[test]
    fn a_metadata_key_is_read_wherever_it_sits() {
        let metadata = "[Application]\nname=org.example.One\nruntime=org.gnome.Platform/x86_64/49\n\n[Context]\nshared=network;\n";
        assert_eq!(
            value_in(metadata, "runtime"),
            "org.gnome.Platform/x86_64/49"
        );
        assert_eq!(value_in(metadata, "name"), "org.example.One");
        assert_eq!(
            value_in(metadata, "sdk"),
            "",
            "a key that is not there came back as something"
        );
    }

    #[test]
    fn the_two_refusals_worth_naming_are_named() {
        // Said in the session's language, so the sentences are asked of the
        // catalog rather than written out here; the English and the Polish of
        // one of them is named in the test below.
        assert_eq!(
            tidy("Error: Not authorized to perform operation"),
            crate::i18n::text("permission-not-given"),
            "a refused password still read as a library error"
        );
        let reaching = crate::message!("could-not-reach-the-remote", "trouble" => "");
        assert!(
            tidy("Failed to resolve host dl.flathub.org").starts_with(reaching.trim_end()),
            "no network did not read as no network"
        );
        assert!(
            tidy("Operation was cancelled").starts_with(crate::i18n::text("stopped")),
            "a job somebody stopped on purpose read as a fault"
        );
        assert!(
            tidy("Aborted by user").starts_with(crate::i18n::text("stopped")),
            "a transaction refused before it began read as a fault"
        );
        // **The one that was the bug.** libflatpak says "aborted" whichever
        // way a transaction gave up, and reading that as Stop told somebody
        // who had pressed nothing that they had pressed something — with the
        // real reason nowhere on the page. See `run_it`, where a failed
        // operation is now carried past and named.
        assert!(
            !tidy("Aborted due to failure").starts_with(crate::i18n::text("stopped")),
            "an operation that failed still reads as somebody pressing Stop"
        );
        assert_eq!(
            tidy("something else entirely"),
            "something else entirely",
            "a message that was already plain was rewritten"
        );
    }

    #[test]
    fn a_job_says_what_it_is_doing_and_what_it_is_doing_it_to() {
        let job = Job::Install {
            scope: Scope::User,
            remote: "flathub".into(),
            reference: "app/org.videolan.VLC/x86_64/stable".into(),
        };
        assert_eq!(job.doing(), crate::i18n::text("installing"));
        assert_eq!(job.about(), "app/org.videolan.VLC/x86_64/stable");
        assert_eq!(job.scope(), Scope::User);
        assert!(job.shown(), "an install would run with no bar at all");
        assert!(
            !Job::Weigh {
                scope: Scope::User,
                remote: "flathub".into(),
                reference: "app/org.videolan.VLC/x86_64/stable".into(),
            }
            .shown(),
            "working out a size would put a bar on the screen and take it away again"
        );
        assert_ne!(
            Job::UpdateAll { scope: Scope::User }.about(),
            Job::UpdateAll {
                scope: Scope::System
            }
            .about(),
            "updating everything for one user and for the machine are one job"
        );
        assert!(
            !Scope::User.goes_through_the_helper(),
            "one user's own installation started going through the system helper"
        );
        assert!(
            Scope::System.goes_through_the_helper(),
            "the machine's installation stopped going through the system helper"
        );
        // And whether the helper will *ask* is polkit's answer, not this one's:
        // an update never needs authorization, whatever the scope.
        assert!(
            !will_ask(Act::Update) || will_ask(Act::Install),
            "a machine that will not ask before installing asked before updating"
        );
    }

    /// The worker's whole path, without installing anything.
    ///
    /// A reference nothing offers is refused while the transaction is being
    /// built, so this exercises the thread, both channels and the reporting of
    /// a failure — and touches neither the disk nor the network.
    /// One that will not go is not a reason to say nothing about the nine
    /// that did, and it has to be *named*: trying them one at a time until
    /// one of them speaks is what this store made somebody do.
    #[test]
    fn what_would_not_go_is_named_and_the_rest_is_not_disowned() {
        assert_eq!(what_would_not_go(11, &[]), "");

        // Nine of eleven: the count of what did go is what says this was not
        // a run in which nothing happened.
        let mostly = what_would_not_go(
            9,
            &[
                "Updating RPCS3 — no such ref".to_string(),
                "Updating Bottles — no such ref".to_string(),
            ],
        );
        assert!(mostly.contains('9'), "{mostly}");
        assert!(mostly.contains('2'), "{mostly}");
        assert!(
            mostly.contains("RPCS3"),
            "the first was not named: {mostly}"
        );
        assert!(mostly.contains("no such ref"), "no reason given: {mostly}");

        // One thing, which did not go: no arithmetic, just what happened.
        let alone = what_would_not_go(0, &["Updating RPCS3 — no such ref".to_string()]);
        assert_eq!(alone, "Updating RPCS3 — no such ref");
    }

    #[test]
    fn a_job_that_cannot_be_done_is_reported_rather_than_lost() {
        let worker = Worker::start();
        worker.send(Job::Install {
            scope: Scope::User,
            remote: "no-such-remote".into(),
            reference: "app/invalid.Nothing.AtAll/x86_64/stable".into(),
        });

        let waited = std::time::Instant::now();
        let mut said = Vec::new();
        while waited.elapsed() < std::time::Duration::from_secs(30) {
            said.extend(worker.heard());
            if said
                .iter()
                .any(|report| matches!(report, Report::Failed { .. } | Report::Done { .. }))
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let ending = said
            .iter()
            .find(|report| matches!(report, Report::Failed { .. } | Report::Done { .. }))
            .unwrap_or_else(|| {
                panic!("the worker said nothing at all in thirty seconds: {said:?}")
            });
        match ending {
            Report::Failed { about, why } => {
                assert_eq!(
                    about, "app/invalid.Nothing.AtAll/x86_64/stable",
                    "the failure named a job nobody sent"
                );
                assert!(!why.is_empty(), "a failure was reported with nothing said");
            }
            other => panic!("a reference nothing offers was accepted: {other:?}"),
        }
    }

    /// What a real install would do, without doing any of it.
    ///
    /// The transaction is resolved and then refused at `ready`, which is the
    /// last moment before anything is fetched. Run it by hand:
    /// `cargo test -- --ignored dry_run`.
    #[test]
    #[ignore]
    fn a_dry_run_says_what_an_install_would_fetch() {
        let reference = std::env::var("DISTRIBUMPY_TRY")
            .unwrap_or_else(|_| "app/app.devsuite.Schemes/x86_64/stable".into());
        let installation = Scope::User.open().expect("a user installation");
        let transaction =
            libflatpak::Transaction::for_installation(&installation, Cancellable::NONE)
                .expect("a transaction");
        transaction.add_default_dependency_sources();
        transaction
            .add_install("flathub", &reference, &[])
            .expect("the reference resolves");

        transaction.connect_ready(|transaction| {
            let mut total = 0;
            for operation in transaction.operations() {
                total += operation.download_size();
                println!(
                    "  {:?} {} — download {} installed {}",
                    operation.operation_type(),
                    operation.get_ref().unwrap_or_default(),
                    size(operation.download_size()),
                    size(operation.installed_size()),
                );
            }
            println!("TOTAL DOWNLOAD: {}", size(total));
            false
        });

        let refused = transaction.run(Cancellable::NONE);
        println!("aborted as intended: {refused:?}");
    }

    /// The whole path, against a real remote: install, see it, remove it.
    ///
    /// Deliberately not part of the ordinary suite — it fetches from Flathub
    /// and writes to the user installation. It leaves the machine as it found
    /// it. Run it by hand:
    /// `cargo test -- --ignored --nocapture a_real_install`.
    ///
    /// `app.devsuite.Schemes` is the subject because it is 828 kB with the
    /// runtime already on this machine, and because nothing else here depends
    /// on it.
    #[test]
    #[ignore]
    fn a_real_install_arrives_is_seen_and_is_taken_away_again() {
        let id = "app.devsuite.Schemes";
        let reference = format!("app/{id}/x86_64/stable");

        let before = Machine::read();
        assert!(
            before.installed_app(id).is_none(),
            "{id} is already installed; this test refuses to touch it"
        );

        let worker = Worker::start();
        worker.send(Job::Install {
            scope: Scope::User,
            remote: "flathub".into(),
            reference: reference.clone(),
        });
        let said = wait_for_the_end(&worker, "installing");
        assert!(
            said.iter().any(|one| matches!(one, Report::Step { .. })),
            "an install reported no step at all: {said:?}"
        );
        assert!(
            said.iter()
                .any(|one| matches!(one, Report::Progress { .. })),
            "an install reported no progress at all: {said:?}"
        );
        assert!(
            matches!(said.last(), Some(Report::Done { .. })),
            "the install did not finish: {said:?}"
        );

        let after = Machine::read();
        let installed = after
            .installed_app(id)
            .unwrap_or_else(|| panic!("{id} installed cleanly and then was not there"));
        assert_eq!(installed.scope, Scope::User, "it went to the wrong place");
        assert!(installed.size > 0, "it arrived with no size at all");
        println!(
            "installed {} {} ({}) in {:?}",
            installed.name,
            installed.version,
            size(installed.size),
            installed.scope
        );

        worker.send(Job::Remove {
            scope: Scope::User,
            reference,
        });
        let said = wait_for_the_end(&worker, "removing");
        assert!(
            matches!(said.last(), Some(Report::Done { .. })),
            "the removal did not finish: {said:?}"
        );

        assert!(
            Machine::read().installed_app(id).is_none(),
            "{id} was left behind on this machine"
        );
        println!("removed it again; the machine is as it was");
    }

    /// The whole repository path, against a real remote: add one, switch it
    /// off, switch it back on, and forget it again.
    ///
    /// Deliberately not part of the ordinary suite — it writes to this user's
    /// flatpak configuration and fetches a catalogue. It leaves the machine as
    /// it found it, and it refuses to run at all if the repository it uses is
    /// already configured. Run it by hand:
    /// `cargo test -- --ignored --nocapture a_repository_is_added`.
    #[test]
    #[ignore]
    fn a_repository_is_added_switched_off_and_on_and_forgotten_again() {
        let name = "distribumpy-test-beta";
        let url = "https://dl.flathub.org/beta-repo/flathub-beta.flatpakrepo";

        let before = Machine::read();
        assert!(
            before.remote(name, Scope::User).is_none(),
            "{name} is already configured; this test refuses to touch it"
        );

        let worker = Worker::start();
        let send = |job: RepoJob| {
            worker.send(Job::Repository {
                scope: Scope::User,
                job,
            });
        };

        send(RepoJob::Add {
            name: name.into(),
            url: url.into(),
        });
        let said = wait_for_the_end(&worker, "adding a repository");
        assert!(
            matches!(said.last(), Some(Report::Done { .. })),
            "adding a repository did not finish: {said:?}"
        );

        let after = Machine::read();
        let added = after
            .remote(name, Scope::User)
            .unwrap_or_else(|| panic!("{name} was added and then was not there"));
        assert!(!added.disabled, "it arrived switched off");
        assert!(
            added.worth_listing(),
            "it arrived as something a page would never show"
        );
        println!("added {} at {}", added.shown(), added.url);

        send(RepoJob::Enable {
            name: name.into(),
            on: false,
        });
        assert!(
            matches!(
                wait_for_the_end(&worker, "switching a repository off").last(),
                Some(Report::Done { .. })
            ),
            "switching a repository off did not finish"
        );
        let off = Machine::read();
        assert!(
            off.remote(name, Scope::User)
                .is_some_and(|one| one.disabled),
            "it was switched off and stayed on"
        );
        assert!(
            !off.catalogue_remotes().iter().any(|one| one.name == name),
            "a repository switched off was still going to be read"
        );

        send(RepoJob::Enable {
            name: name.into(),
            on: true,
        });
        wait_for_the_end(&worker, "switching a repository on");
        assert!(
            Machine::read()
                .remote(name, Scope::User)
                .is_some_and(|one| !one.disabled),
            "it was switched back on and stayed off"
        );

        send(RepoJob::Forget { name: name.into() });
        assert!(
            matches!(
                wait_for_the_end(&worker, "forgetting a repository").last(),
                Some(Report::Done { .. })
            ),
            "forgetting a repository did not finish"
        );
        assert!(
            Machine::read().remote(name, Scope::User).is_none(),
            "{name} was left behind on this machine"
        );
        println!("switched it off, on, and forgot it again; the machine is as it was");
    }

    #[cfg(test)]
    fn wait_for_the_end(worker: &Worker, doing: &str) -> Vec<Report> {
        let waited = std::time::Instant::now();
        let mut said = Vec::new();
        while waited.elapsed() < std::time::Duration::from_secs(600) {
            for report in worker.heard() {
                if let Report::Step { what, at, of } = &report {
                    println!("  step {at}/{of}: {what}");
                }
                let ending = matches!(report, Report::Done { .. } | Report::Failed { .. });
                said.push(report);
                if ending {
                    return said;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("nothing finished {doing} in ten minutes: {said:?}");
    }
}
