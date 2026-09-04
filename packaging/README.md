# DistriBumpy packaging

These definitions install Software Hub system-wide, so that somebody can run it
on a machine that has never seen this checkout. One package comes out of one
source tree on every distribution here:

```text
distribumpy   bin/distribumpy
              share/applications/distribumpy.desktop
              share/icons/hicolor/scalable/apps/distribumpy.svg
              share/metainfo/io.github.petexy.distribumpy.metainfo.xml
```

Nothing is split. lxb-toolkit splits a library from its header because two
different machines want two different halves of it; this is one executable, and
splitting it from its own desktop entry would produce a package that installs
and then cannot be started.

Every recipe calls `packaging/install.sh` rather than listing those four files
itself, so adding one file is one edit instead of five that have to agree. What
install.sh does *not* place is the licence and the documentation: where those go
is the one thing the distributions really disagree about — Arch and Fedora want
`/usr/share/licenses`, Debian wants a copyright file under `/usr/share/doc` —
so each recipe places its own.

## The toolkit is a build dependency, not a runtime one

This is the thing about packaging DistriBumpy that looks like a mistake and is
not. `lxb-app` is a Rust **path** dependency:

```toml
lxb-app = { version = "0.3.11", path = "/usr/share/lxb-toolkit/crates/lxb-app" }
```

Cargo compiles those sources into this binary. The finished program links no
`liblxb_*.so` at all — `ldd` on it names libflatpak, glib, wayland and libc, and
nothing of the toolkit — so a machine that *runs* Software Hub needs nothing of
lxb-toolkit installed, and a machine that *builds* it needs the crate sources
that the toolkit's devel component puts under `/usr/share/lxb-toolkit/crates`.

So: `makedepends` on Arch, `BuildRequires: lxb-toolkit-devel` on Fedora,
`Build-Depends: lxb-toolkit-dev` on Debian, and nothing in `depends` anywhere.
Nix is the exception and needs no installed toolkit at all: it takes the
derivation as an input and rewrites that absolute path to a store path.

The version is asked of `Cargo.toml` rather than repeated. The Arch and Debian
definitions are rendered from `.in` templates and carry `@TOOLKIT_VERSION@`; the
spec has to be a literal, because rpmbuild reads it where it stands, and
`build.sh check` is what compares it against the manifest.

## What is opened at run time and cannot be seen

`libxkbcommon.so.0`, `libEGL.so.1` and `libwayland-egl.so.1` are loaded by name
at run time rather than linked, so nothing that reads the ELF — not
`dpkg-shlibdeps`, not rpm's dependency generator, not `ldd` — will ever name
them. Every recipe here names them by hand. A package that leaves them out
installs cleanly and then dies with a message about a missing library the first
time somebody opens it.

Vulkan is different again: with a driver present wgpu draws through it, and
without one it falls back to EGL. So the loader is *suggested* rather than
required, and the EGL path is what makes that honest.

## One version, in one file

The version this project releases under is the single line in `VERSION` at the
root of the checkout, and what a package claims and what `distribumpy --version`
reports are the same number because both come from there.

Most things read that file where it stands: the Arch, Debian and Nix
definitions, and the source archive's name. Three cannot read a file and carry
the number as a literal instead — `[package]` in `Cargo.toml`, which is where
`--version` gets it; `Version:` in the Fedora spec, which has to be a literal
for the spec to be one anyone could submit; and the AppStream release list,
which is not only a version but the entry every other software centre reads to
say what this release changed. A release is therefore one command that writes
them from the file:

```sh
./scripts/bump-version.sh 0.2.0
```

`packaging/build.sh check` refuses to let any of them drift apart.

## Where the build happens, and why not /tmp

Every builder works under `packaging/out/build/`, on whatever filesystem the
checkout is on. Not `${TMPDIR:-/tmp}`, which is the obvious choice and the wrong
one: on a systemd machine /tmp is a tmpfs sized at a fraction of RAM, so
building there means building in memory.

This is a GPU application. The locked graph is 400-odd crates with wgpu and naga
in it, a release build measures about 2.3 GiB, and the `cargo test` that
makepkg's `check()` and rpmbuild's `%check` run builds the whole of it again in
the dev profile. There is no laptop where that fits in a tmpfs.

Send it elsewhere with `--work-dir DIR` on the Arch and Fedora builders, or
`DISTRIBUMPY_WORK_DIR` for all of them:

```sh
./packaging/build.sh arch --work-dir /var/tmp/distribumpy
DISTRIBUMPY_WORK_DIR=/var/tmp/distribumpy ./packaging/build.sh fedora
```

A work directory inside the checkout is refused unless it is under
`packaging/out`, because `snapshot_source` picks up untracked files and a build
tree anywhere else would end up inside the source archive built from it.

The builders check free space before extracting anything, so a machine without
the room is told immediately rather than partway through. That check reads `df`,
which cannot see a quota — the default location is what actually solves the
quota case. One thing it cannot route around either: `makepkg.conf` wins over
the environment, so a machine that sets `BUILDDIR` builds there whatever
`--work-dir` said. The Arch builder notices and says so.

## Validate the definitions and their payload

```sh
./packaging/build.sh check
```

This checks shell syntax, confirms every package definition still takes its
version from `VERSION` and its toolkit requirement from `Cargo.toml`, confirms
the desktop entry, the AppStream component and the window title call this
application the same thing, builds the release binary, confirms it reports the
packaged version, stages the payload, proves nothing in `data/` has quietly
stopped shipping, proves the desktop entry's `Exec` and `Icon` name things the
package actually installs, and hands both metadata files to
`desktop-file-validate` and `appstreamcli`. Use `--no-build` when a current
release binary already exists.

## Arch Linux

```sh
./packaging/build.sh arch
./packaging/build.sh arch -- --syncdeps
./packaging/build.sh arch -- --nocheck
```

The wrapper creates a deterministic source archive and renders a PKGBUILD with
its real SHA-256 checksum before running `makepkg`. Artifacts are copied to
`packaging/out/arch/`.

## Debian

Build on Debian, Ubuntu, or another Debian-derived system with `dpkg-dev`:

```sh
./packaging/build.sh debian
```

The builder uses `dpkg-shlibdeps` on the locally linked binary, stages one
policy-shaped binary package, and writes it to `packaging/out/debian/`.

`--allow-foreign-host` exists for package-structure testing only, and says so
twice: a `.deb` built against another distribution's libc must not be deployed
on Debian, and on a host with no dpkg database there is nothing to resolve
shared libraries against, so the package comes out with no generated `Depends`
at all and the builder warns. What such a build is good for is looking at the
shape — which files landed where, and what the package declares.

`lxb-toolkit-dev` is not in Debian. Until it is, build it from the toolkit's own
`packaging/build.sh debian` and install it first.

## Fedora

```sh
./packaging/build.sh fedora
./packaging/build.sh fedora --no-check
```

The builder snapshots the working tree, runs `cargo vendor --locked`, and
creates an offline source archive before invoking `rpmbuild -ba`. Binary and
source RPMs are copied to `packaging/out/fedora/`.

The toolkit's crates are deliberately *not* vendored: they are a path dependency
at an absolute path, and `cargo vendor` leaves those where they are. That is why
the spec build-requires `lxb-toolkit-devel` — the sources have to be under
`/usr/share` on the builder, exactly as they are here.

The spec disables the debuginfo subpackage, because Cargo's release profile
emits no DWARF for `find-debuginfo` to collect. Submitting to the Fedora archive
means reversing that: build with `-Cdebuginfo=2 -Cstrip=none` under Fedora's own
path remapping and drop `%global debug_package %{nil}`.

## Nix

```sh
./packaging/build.sh nix
nix build path:.#distribumpy
nix run path:.#distribumpy
nix-build packaging/nix --arg lxb-toolkit-src ~/GitHub/lxb-toolkit
```

The flake takes lxb-toolkit as an input and makes its nixpkgs follow this one,
so the toolkit's crates and this program are compiled by one rustc. `postPatch`
rewrites the `/usr/share/lxb-toolkit/crates` in `Cargo.toml` to that store path
— the one line here that turns an FHS assumption into a Nix one — and the lock
file is untouched by it, because a path dependency carries no source there.

The non-flake `default.nix` fetches the toolkit from its repository by default;
`--arg lxb-toolkit-src` points it at a local checkout instead, which is the
useful form while both are being worked on.

## After installing

```sh
distribumpy            # opens a window
distribumpy --version
distribumpy --shot page.png --shelf 1   # a settled frame, with no display
```

These recipes are intended for local and CI packages during early development.
Before submission to an official distribution archive, build in that
distribution's clean builder and complete its dependency-license review.
