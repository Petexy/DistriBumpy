Name:           distribumpy
Version:        0.9.0
Release:        1%{?dist}
Summary:        A Flatpak store in the LineXinBar design language, shown as Software Hub

# This program, and the locked Rust dependency graph vendored into the source
# archive. Every crate offering a choice is taken under its permissive option:
# self_cell as Apache-2.0 rather than GPL-2.0-only, r-efi as MIT rather than
# LGPL-2.1-or-later. What is left after that choice is this list, and it is
# derived from the lock file rather than remembered.
License:        GPL-3.0-only AND Apache-2.0 AND MIT AND Apache-2.0 WITH LLVM-exception AND BSD-2-Clause AND BSD-3-Clause AND ISC AND MPL-2.0 AND Unicode-3.0 AND Unlicense AND Zlib AND 0BSD AND CDLA-Permissive-2.0
URL:            https://github.com/Petexy/distribumpy
Source0:        distribumpy-%{version}.tar.gz

ExclusiveArch:  x86_64 aarch64

# Cargo's release profile emits no DWARF, so find-debuginfo would produce an
# empty debugsourcefiles.list and rpmbuild would fail on it after the whole
# build. An archive submission wants real debuginfo instead: drop this, and
# with it the -Cdebuginfo=0 in %build that holds Fedora's own -Cdebuginfo=2 off,
# so the DWARF is built and packaged rather than built and binned.
%global debug_package %{nil}

BuildRequires:  cargo >= 1.87
BuildRequires:  rust >= 1.87
BuildRequires:  gcc
BuildRequires:  pkgconfig
BuildRequires:  pkgconfig(flatpak)
BuildRequires:  pkgconfig(glib-2.0)
BuildRequires:  desktop-file-utils
BuildRequires:  libappstream-glib
# The design language, as Rust sources. It is a build dependency and not a
# runtime one: `lxb-app` is a path dependency, so cargo compiles it into this
# binary and the finished program links no liblxb_*.so at all.
BuildRequires:  lxb-toolkit-devel >= 0.9.0

Requires:       flatpak
# Opened by name at run time rather than linked, so rpm's automatic dependency
# generator cannot see either of them in the ELF.
Requires:       libxkbcommon
Requires:       libglvnd-egl
# The Links page hands an address to whatever this desktop opens addresses
# with, which is the only thing here not done in-process.
Recommends:     xdg-utils
# With a Vulkan driver present this draws through it; without one it falls
# back to EGL, so the loader is worth having and is not required.
Suggests:       vulkan-loader

%description
A Flatpak store drawn in the LineXinBar design language: the same colours,
glass, motion and marks as the shell it was made for, and driven from a
keyboard, a pointer, a touchscreen and a controller at once. It is an ordinary
Wayland application and runs under GNOME or Plasma as readily as under that
shell.

It installs, updates and removes through libflatpak — the same library the
flatpak command is a front end for — so a transaction here is the transaction
that command would have run: resolved dependencies, progress operation by
operation, and a cancel that works. The catalogue it browses is the AppStream
data every remote already keeps on the disk, so a machine that has been updated
once can be browsed with the network unplugged.

%prep
%autosetup -n distribumpy-%{version}

%build
export RUSTUP_TOOLCHAIN=stable
export CARGO_TARGET_DIR=target
# Fedora exports its own %%{build_rustflags} into RUSTFLAGS before this runs, and
# they carry -Cdebuginfo=2 -Cstrip=none. RUSTFLAGS is appended after the release
# profile's own flags and wins, so every crate in the graph was generating full
# DWARF — and with %%global debug_package %%{nil} above, no package was ever made
# of it. -Cdebuginfo=0 last is what turns that back off. It is worth about
# 274 MiB of resident memory on the final rustc here, measured: 1572 MiB with
# the DWARF, 1298 MiB without.
export RUSTFLAGS="${RUSTFLAGS:-} -Cdebuginfo=0"

# And Cargo takes its job count from the core count alone, knowing nothing about
# how much memory the machine has to hold that many rustc at once. wgpu and naga
# are in this graph and thin LTO with one codegen unit is what the release
# profile asks for, so the count has to answer to memory as well. The sister
# repository's shell was killed by the kernel's OOM killer twice on an 8 GiB
# Apple M1 for want of exactly this.
#
# Arithmetic rather than %%limit_build, the Fedora macro meant for this, which
# swallowed the remainder of the script it was used in on Fedora Asahi.
build_jobs="%{_smp_build_ncpus}"
build_room="$(awk '/^MemTotal:/ { n = int($2 / 1024 / 2048); print (n < 1 ? 1 : n) }' /proc/meminfo 2>/dev/null || true)"
if [ -n "$build_room" ] && [ "$build_room" -lt "$build_jobs" ]; then
    build_jobs="$build_room"
fi
echo "building with $build_jobs of %{_smp_build_ncpus} jobs, for the memory this machine has"
cargo build --offline --locked --release -j"$build_jobs"

%install
export CARGO_TARGET_DIR=target
./packaging/install.sh \
    --destdir %{buildroot} \
    --prefix %{_prefix} \
    --target-dir target

%check
export RUSTUP_TOOLCHAIN=stable
export CARGO_TARGET_DIR=target
# The same two as %%build. The dev profile asks for full DWARF and this phase
# builds the graph a second time to get it, with no package made of it either;
# a failing test still names its file and line, which the panic carries rather
# than DWARF.
export RUSTFLAGS="${RUSTFLAGS:-} -Cdebuginfo=0"
build_jobs="%{_smp_build_ncpus}"
build_room="$(awk '/^MemTotal:/ { n = int($2 / 1024 / 2048); print (n < 1 ? 1 : n) }' /proc/meminfo 2>/dev/null || true)"
if [ -n "$build_room" ] && [ "$build_room" -lt "$build_jobs" ]; then
    build_jobs="$build_room"
fi
cargo test --offline --locked -j"$build_jobs"
# The two files that are read by something other than this program. Both are
# installed by then, so what is checked is what ships rather than what is in
# the checkout.
desktop-file-validate %{buildroot}%{_datadir}/applications/distribumpy.desktop
appstream-util validate-relax --nonet \
    %{buildroot}%{_metainfodir}/io.github.petexy.distribumpy.metainfo.xml

%files
%license LICENSE
%doc README.md
%{_bindir}/distribumpy
%{_datadir}/applications/distribumpy.desktop
%{_datadir}/icons/hicolor/scalable/apps/distribumpy.svg
%{_metainfodir}/io.github.petexy.distribumpy.metainfo.xml

%changelog
* Sun Aug 30 2026 Piotr Lewandowski <piotr.petexy@gmail.com> - 0.9.0-1
- LineXinBar, lxb-toolkit, CEDM and this store now release under one version,
  so that what somebody has installed can be read off one number rather than
  four that drift apart.
- The application is shown as Software Hub. It was "Store", which names a
  category rather than a thing.
- Packaging for Arch, Debian, Fedora and Nix, all staging one payload through
  packaging/install.sh.
- Requires lxb-toolkit 0.9.0 to build. That is 0.3.11's code renumbered, but a
  caret requirement on a 0.x version pins the minor, so the older toolkit will
  no longer satisfy it.

* Sun Aug 30 2026 Piotr Lewandowski <piotr.petexy@gmail.com> - 0.1.0-1
- First packaged release. A Flatpak store shown as Software Hub: Flathub's own
  collections on the Home page, every remote's catalogue read off the disk, and
  install, update and remove through libflatpak with progress per operation.
