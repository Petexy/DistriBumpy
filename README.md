# DistriBumpy

**A Flatpak store for [LineXinBar](https://github.com/Petexy/LineXinBar), driven
by a controller. It is shown as *Software Hub*.**

[![Licence](https://img.shields.io/badge/licence-GPL--3.0--only-blue)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.9.0-informational)](VERSION)
[![Rust](https://img.shields.io/badge/rust-1.90%2B-orange)](Cargo.toml)

![The Home page](docs/home.png)

`distribumpy` is the project, the package and the command; **Software Hub** is
what it is called on the desktop entry, in the menu and on the window. It is
built on [lxb-toolkit](https://github.com/Petexy/lxb-toolkit) — the shell's own
colours, glass, motion, type and marks — and it is an ordinary Wayland
application, so it runs under GNOME or Plasma as readily as under the shell it
was made for. The wheel and a touchpad scroll whichever pane is under the
pointer, without first making somebody move focus into it.

Its siblings are [Videonsole](https://github.com/Petexy/videonsole) (Videos),
[Imagonsole](https://github.com/Petexy/imagonsole) (Pictures) and
[SongOnSole](https://github.com/Petexy/songonsole) (Music).

## What it does

- **Opens on a Home page** with one featured application, then what Flathub
  itself says is worth looking at: what is asked for most, what is rising
  fastest, what has just been published and what has just been rebuilt. Those
  four lists are the one thing here that cannot be read off this disk, so they
  are kept after they are fetched and the page comes up filled with no network.
  **Flathub publishes no ratings** — there is no score, no stars and no reviews
  in its API — so nothing here invents one.
- **Browses every repository flatpak knows about**, from the AppStream
  catalogue already on the disk. Flathub's is 47 MB, read as a stream on a
  thread of its own, so a machine that has been updated once can be browsed
  with the network unplugged.
- **Shelves that match the shell's own** — Games, Multimedia, Graphics,
  Internet, Office, Development, Education & Science, Utilities, System — so
  something found here lands on the column it will appear in.
- **Searches by name and by summary.** Nothing is called "video editor", but
  Kdenlive's summary is exactly that, so a query of more than one word answers
  with what it means rather than with what is first in the alphabet.
- **Installs, updates and removes**, with progress operation by operation — a
  transaction installing one application routinely runs a dozen — and a
  **Stop** that keeps whatever has already been fetched. Removing an
  application or forgetting a repository first opens a confirmation with the
  safe answer selected.
- **Says what an install would really cost** before the press: the download and
  the disk, counting every runtime and extension this machine does not already
  have. That is the difference between 828 kB and 759 MB, and it can only be
  learnt by resolving the transaction and refusing it.
- **Updates everything**, and **clears out the runtimes nothing needs any
  more**, which is where a machine's flatpak disk usage actually goes.
- **Opens what is installed**, without going back to a menu for it.

![The shelves and what is on them](docs/browse.png)

**A list says it runs on by dissolving at its ends**: over the last card and a
bit, its cards and their words grow blurrier and more translucent together
until what is left is exactly the page behind them — so the list stops without
anything to stop at, while the heading, the shelves and the footer stay sharp.
Everything on offer is a card in a grid three across, as many as fit, down to
one on a narrow window.

![One application](docs/detail.png)

An application's page carries publisher, verification, version, size, install
scope, runtime and licence as facts that wrap rather than disappear off a
narrow window. It also carries what changed in each release, everywhere else to
read about it, and its screenshots — each in a frame cut to the shape the
catalogue declares for it, so the frame is right before the picture has been
fetched.

### Permissions

![What an application may reach](docs/permissions.png)

What each application may reach outside its sandbox, and a switch for every one
of it that is worth switching. Anything it asked for that has no switch here is
still listed, so nothing it asked for is hidden.

Changes are written to **this user's own override**, in
`~/.local/share/flatpak/overrides/`, as differences rather than as a whole
permission set: `!network` takes something away that the application asked for,
and setting a permission back removes the entry entirely. That is what makes an
override safe to keep across an update that changes what the application asks
for — and it is why nothing here needs a password, even for an application
installed for the whole machine.

### Repositories, and where things go

Every repository of both installations can be **switched off** without being
forgotten, have its **catalogue fetched** again, or be **forgotten** entirely.
Flathub, Flathub beta, GNOME Nightly and KDE Nightly can be added with one
press, or anything else by the address of its `.flatpakrepo` file.

New applications are installed **for the current user** wherever that
repository is available there, so nothing has to be authorised and nothing
outside `~/.local/share` changes. Anything already installed **for the whole
system** is listed, updated and removed just as readily; those raise the
desktop's own password panel, and the page says so before the press rather than
after it.

## Controls

A controller, a keyboard and a pointer are one interface rather than three.
What the buttons do is written in the corner opposite, drawn rather than
lettered: the same act is South on a pad and Enter on a keyboard, and the
corner says whichever the shell last saw reached for.

Opening the search shelf stands the light **in the field**, which is what says
a field has the cursor — and under LineXinBar that is what brings the on-screen
keyboard up for it. Back leaves the field for the shelves.

![Searching by what a thing is for](docs/search.png)

## Install

Rust 1.90 or newer, and the **lxb-toolkit development component** —
`Cargo.toml` names its crate sources at `/usr/share/lxb-toolkit/crates`, and
cargo compiles them into this binary, so nothing of the toolkit is linked at
run time. Beside that: **flatpak** and **glib** with their development files,
which `libflatpak` links against.

```sh
cargo build --release --locked
sudo ./packaging/install.sh --destdir / --prefix /usr
```

`install.sh` places the binary, the desktop entry, the icon and the AppStream
data, and nothing else. For a user-local install instead, with `~/.local/bin`
on `PATH`:

```sh
./packaging/install.sh --destdir / --prefix "$HOME/.local"
```

### As a package

Every recipe calls that same `install.sh`, so a package cannot quietly ship a
different set of files from the line above.

```sh
./packaging/build.sh check     # what a package would have to agree with
./packaging/build.sh arch      # makepkg
./packaging/build.sh debian    # dpkg-deb, on Debian or Ubuntu
./packaging/build.sh fedora    # rpmbuild, on Fedora
./packaging/build.sh nix       # the flake — the one target that does not
                               # need lxb-toolkit installed already
```

Or with Nix and no checkout at all:

```sh
nix run github:Petexy/distribumpy
```

See [`packaging/README.md`](packaging/README.md) for why the toolkit is a
*build* dependency and not a runtime one, and for the two libraries that are
opened by name at run time and so must be named by hand in every package.

## Verify

```sh
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo fmt --check
```

Four integration tests exercise Flatpak or its per-user override path and are
left out of the ordinary run. All leave the machine exactly as they found it:

```sh
cargo test -- --ignored --nocapture a_real_install         # install and remove
cargo test -- --ignored --nocapture a_repository_is_added  # add, switch, forget
cargo test -- --ignored --nocapture a_dry_run              # what an install would fetch
cargo test -- --ignored --test-threads 1 an_override_really
```

`--shot` writes one settled frame to a PNG **with no display at all**, through
the same page function and the same renderer the window uses. Every animation
is put where it is going first, so what it photographs is the page at rest.
Unlike its three siblings this one has no `--demo`: it reads the machine it is
run on, because a store that made up its catalogue would be a picture of the
made-up catalogue.

```sh
distribumpy --shot page.png --shelf 5 --width 1600 --height 900
distribumpy --shot detail.png --app org.videolan.VLC --tab 0 --width 1600 --height 900
```

Every picture in this README was taken that way. Shelves count from Home at
nought.

## Languages

Ten, compiled in: German, English (UK), English (US), Spanish, French, Hindi,
Polish, Brazilian Portuguese, Russian and Simplified Chinese — in whichever one
the session speaks, which on LineXinBar is the one Settings ▸ Language names.
Flathub's own names, summaries and release notes are shown in that language too
where the remote publishes them. See [localization](docs/localization.md).

## How it is put together

| | |
|---|---|
| `src/catalogue.rs` | The AppStream catalogue: a streaming parse, the shelves, and the search |
| `src/flatpak.rs` | libflatpak: the installations, the repositories, and the transaction worker |
| `src/flathub.rs` | The four lists behind the Home page, the only thing not on this disk |
| `src/sandbox.rs` | What an application may reach, and this user's overrides |
| `src/art.rs` | Screenshots — the one part of a listing not already on this disk |
| `src/motion.rs` | Everything on a page that is on its way somewhere |
| `src/store.rs` | What is on the screen and what the controls do to it |
| `src/draw.rs` | The shelves, the listing, and what runs along the bottom |
| `src/detail.rs` | The pages that are about one thing |
| `src/legend.rs` | What the buttons do, and which control the user has in hand |

**[`docs/design.md`](docs/design.md)** is the long answer: the five threads and
why only one of them draws, why `libflatpak`'s objects never cross between
them, and how a page is photographed on its way out of the card that opened it.

## Licence

[GPL-3.0-only](LICENSE), matching LineXinBar and the toolkit. It releases under
the same version as LineXinBar, lxb-toolkit, Imagonsole, Videonsole,
SongOnSole and CEDM.
