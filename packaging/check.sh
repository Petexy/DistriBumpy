#!/usr/bin/env bash

set -euo pipefail

packaging_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=packaging/lib.sh
source "$packaging_dir/lib.sh"

build=true
case "${1:-}" in
    --no-build) build=false ;;
    -h | --help)
        echo "Usage: packaging/check.sh [--no-build]"
        exit 0
        ;;
    "") ;;
    *) package_die "unknown check option: $1" ;;
esac

# The version bumper lives in scripts/ rather than in here, because the version
# is not a packaging detail — but it is the thing that writes the manifest and
# the spec this file checks, so it is checked with them.
shell_scripts() {
    find "$PACKAGING_DIR" -type f -name '*.sh' -print0
    printf '%s\0' "$PROJECT_ROOT/scripts/bump-version.sh"
}

package_note "checking shell syntax"
while IFS= read -r -d '' script; do
    bash -n "$script"
done < <(shell_scripts)
bash -n "$PACKAGING_DIR/arch/PKGBUILD.in"

if command -v shellcheck >/dev/null 2>&1; then
    while IFS= read -r -d '' script; do
        shellcheck -x "$script"
    done < <(shell_scripts)
else
    package_note "shellcheck is not installed; syntax was checked but not linted"
fi

package_note "checking the package version is consistent"
read_toml_version() {
    local file="$1"
    local wanted_section="$2"
    awk -v wanted="$wanted_section" '
        /^\[/ { section = $0; next }
        section == wanted \
            && match($0, /^version[[:space:]]*=[[:space:]]*"[^"]+"/) {
            line = substr($0, RSTART, RLENGTH)
            sub(/^version[[:space:]]*=[[:space:]]*"/, "", line)
            sub(/"$/, "", line)
            print line
            exit
        }' "$file"
}

# `distribumpy --version` prints CARGO_PKG_VERSION, so the manifest is where a
# running program says what it is.
manifest_version="$(read_toml_version "$PROJECT_ROOT/Cargo.toml" "[package]")"
[[ -n "$manifest_version" ]] \
    || package_die "could not read [package] version from Cargo.toml"
[[ "$manifest_version" == "$PACKAGE_VERSION" ]] \
    || package_die "VERSION says $PACKAGE_VERSION but Cargo.toml says $manifest_version.
Run: scripts/bump-version.sh $PACKAGE_VERSION"

# Every packaged build is --locked or --frozen, so a lock file naming the old
# version is not a stale file: it is a build that refuses to start.
locked_version="$(awk '
    /^name = "distribumpy"$/ { found = 1; next }
    found && match($0, /^version = "[^"]+"/) {
        line = substr($0, RSTART, RLENGTH)
        sub(/^version = "/, "", line)
        sub(/"$/, "", line)
        print line
        exit
    }' "$PROJECT_ROOT/Cargo.lock")"
[[ "$locked_version" == "$PACKAGE_VERSION" ]] \
    || package_die "Cargo.lock still says distribumpy $locked_version, not $PACKAGE_VERSION.
Run: scripts/bump-version.sh $PACKAGE_VERSION"

# The spec carries a literal version — `Version:` has to be one for the spec to
# be a spec anyone could submit — so compare it against VERSION itself: a
# pattern spelling out the version would agree with a stale spec forever, and
# the mismatch would only surface as rpmbuild failing to find its Source0.
grep -Eq "^Version:[[:space:]]+${PACKAGE_VERSION//./\\.}\$" \
    "$PACKAGING_DIR/fedora/distribumpy.spec" \
    || package_die "fedora/distribumpy.spec does not declare version $PACKAGE_VERSION"

# AppStream orders releases newest first, and the newest one is what GNOME
# Software and Discover show as this application's version. A release list that
# stops at the version before this one is how a store tells everybody they are
# up to date when they are not.
newest_release="$(awk '
    match($0, /<release version="[^"]+"/) {
        line = substr($0, RSTART, RLENGTH)
        sub(/^<release version="/, "", line)
        sub(/"$/, "", line)
        print line
        exit
    }' "$PROJECT_ROOT/data/io.github.petexy.distribumpy.metainfo.xml")"
[[ "$newest_release" == "$PACKAGE_VERSION" ]] \
    || package_die "the metainfo's newest release is $newest_release, not $PACKAGE_VERSION.
Add a <release> for $PACKAGE_VERSION to data/io.github.petexy.distribumpy.metainfo.xml"

# The rest take the version from VERSION, so check that they still do.
grep -Fqx 'pkgver=@VERSION@' "$PACKAGING_DIR/arch/PKGBUILD.in" \
    || package_die "arch/PKGBUILD.in no longer reads its version from VERSION"
grep -Fq 'builtins.readFile ../../VERSION' "$PACKAGING_DIR/nix/package.nix" \
    || package_die "nix/package.nix no longer reads its version from VERSION"
grep -Fq 'Version: @VERSION@-1' "$PACKAGING_DIR/debian/control.in" \
    || package_die "debian/control.in no longer reads its version from VERSION"

package_note "checking every recipe asks for the same toolkit"
# The toolkit is a build dependency and not a runtime one — cargo compiles
# lxb-app into this binary — but it is a *versioned* build dependency, and the
# version is in Cargo.toml. A recipe naming an older one produces a build that
# fails in cargo with a message about a path, on somebody else's machine.
toolkit_version="$(package_toolkit_requirement)"
# Two of the three are rendered by their builder, so they ask for the
# requirement rather than repeating it. What is checked there is that they
# still do — a literal that crept back in would be right on the day it was
# written and wrong from the next release onwards.
grep -Fq 'lxb-toolkit>=@TOOLKIT_VERSION@' "$PACKAGING_DIR/arch/PKGBUILD.in" \
    || package_die "arch/PKGBUILD.in no longer takes its toolkit requirement from Cargo.toml"
grep -Fq 'lxb-toolkit-dev (>= @TOOLKIT_VERSION@)' "$PACKAGING_DIR/debian/source-control.in" \
    || package_die "debian/source-control.in no longer takes its toolkit requirement from Cargo.toml"
# The spec is rendered by nothing: rpmbuild reads it where it stands, so this
# one is a literal and this is the check that it is the right literal.
grep -Eq "^BuildRequires:[[:space:]]+lxb-toolkit-devel >= ${toolkit_version//./\\.}\$" \
    "$PACKAGING_DIR/fedora/distribumpy.spec" \
    || package_die "fedora/distribumpy.spec does not build-require lxb-toolkit-devel >= $toolkit_version.
Cargo.toml asks for lxb-app $toolkit_version, and the spec has to say so."

package_note "checking the shown name agrees everywhere"
# Three places name this application to a person, and they are read by three
# different things: the desktop entry by every menu, the AppStream component by
# every other software centre, and the string handed to the toolkit by the
# window itself. Two of them agreeing is what a rename looks like when it is
# half done, and nothing else notices.
desktop_name="$(awk -F= '/^Name=/ { print substr($0, 6); exit }' \
    "$PROJECT_ROOT/data/distribumpy.desktop")"
metainfo_name="$(awk 'match($0, /<name>[^<]+<\/name>/) {
        line = substr($0, RSTART, RLENGTH)
        sub(/^<name>/, "", line)
        sub(/<\/name>$/, "", line)
        print line
        exit
    }' "$PROJECT_ROOT/data/io.github.petexy.distribumpy.metainfo.xml")"
window_names="$(grep -o 'App::new("distribumpy", "[^"]*")' "$PROJECT_ROOT/src/main.rs" \
    | sed -e 's/^.*, "//' -e 's/")$//' | sort -u)"
[[ -n "$desktop_name" ]] || package_die "the desktop entry has no Name="
[[ -n "$metainfo_name" ]] || package_die "the metainfo has no <name>"
[[ "$(printf '%s\n' "$window_names" | wc -l)" -eq 1 ]] \
    || package_die "src/main.rs opens windows under more than one name:
$window_names"
[[ "$desktop_name" == "$metainfo_name" && "$desktop_name" == "$window_names" ]] \
    || package_die "this application is called three things:
  desktop entry  $desktop_name
  metainfo       $metainfo_name
  window title   $window_names"
package_note "it is called '$desktop_name'"

if [[ "$build" == true ]]; then
    package_note "building the release artefact"
    require_rust_version 1.87
    require_toolkit_sources
    (cd "$PROJECT_ROOT" && cargo build --locked --release)
fi

target_dir="${CARGO_TARGET_DIR:-$PROJECT_ROOT/target}"
[[ "$target_dir" == /* ]] || target_dir="$PROJECT_ROOT/$target_dir"

package_note "checking the binary reports the packaged version"
program="$target_dir/release/distribumpy"
[[ -x "$program" ]] || package_die "missing release binary: $program
Run without --no-build, or build it first."
reported="$("$program" --version)"
[[ "$reported" == "distribumpy $PACKAGE_VERSION" ]] \
    || package_die "the binary reports '$reported', not 'distribumpy $PACKAGE_VERSION'"

package_note "staging the payload"
stage="$(package_work_dir distribumpy-check)"
cleanup() {
    if [[ -n "${stage:-}" && "$stage" == */distribumpy-check.* && -d "$stage" ]]; then
        rm -rf -- "$stage"
    fi
}
trap cleanup EXIT

"$PACKAGING_DIR/install.sh" \
    --destdir "$stage" \
    --prefix /usr \
    --target-dir "$target_dir" >/dev/null
staged="$(cd "$stage" && find . -mindepth 1 \( -type f -o -type l \) -printf '%P\n' | sort)"

package_note "checking the payload is complete"
for expected in \
    usr/bin/distribumpy \
    usr/share/applications/distribumpy.desktop \
    usr/share/icons/hicolor/scalable/apps/distribumpy.svg \
    usr/share/metainfo/io.github.petexy.distribumpy.metainfo.xml; do
    printf '%s\n' "$staged" | grep -Fqx "$expected" \
        || package_die "the staged payload is missing $expected"
done

# The list above is what a package promises; this is what the checkout has. A
# file added to data/ and not to install.sh is a file that exists everywhere
# except in the package, which is the one place it matters.
while IFS= read -r file; do
    printf '%s\n' "$staged" | grep -Fq "/$file" \
        || package_die "data/$file is in the checkout but in no package"
done < <(cd "$PROJECT_ROOT/data" && find . -maxdepth 1 -type f -printf '%P\n' | sort)

package_note "checking the desktop entry points at what was installed"
# Exec and Icon are names, not paths, and nothing resolves them until somebody
# clicks. A rename of the binary that missed the desktop file installs cleanly
# and produces an entry that does nothing at all.
exec_name="$(awk -F= '/^Exec=/ { print $2; exit }' \
    "$stage/usr/share/applications/distribumpy.desktop" | awk '{print $1}')"
icon_name="$(awk -F= '/^Icon=/ { print $2; exit }' \
    "$stage/usr/share/applications/distribumpy.desktop")"
[[ -x "$stage/usr/bin/$exec_name" ]] \
    || package_die "the desktop entry runs '$exec_name', which the package does not install"
[[ -f "$stage/usr/share/icons/hicolor/scalable/apps/$icon_name.svg" ]] \
    || package_die "the desktop entry wears '$icon_name', which the package does not install"

package_note "checking the desktop entry is valid"
if command -v desktop-file-validate >/dev/null 2>&1; then
    desktop-file-validate "$stage/usr/share/applications/distribumpy.desktop" \
        || package_die "the installed desktop entry is not valid"
else
    package_note "desktop-file-validate is not installed; the entry was not validated"
fi

package_note "checking the AppStream metadata is valid"
if command -v appstreamcli >/dev/null 2>&1; then
    # `--no-net`: this must pass on a build machine with no network, and what
    # the network would add is a check of somebody else's screenshot host.
    appstreamcli validate --no-net --pedantic \
        "$stage/usr/share/metainfo/io.github.petexy.distribumpy.metainfo.xml" \
        || package_die "the installed metainfo does not validate"
else
    package_note "appstreamcli is not installed; the metadata was not validated"
fi

# And the Fedora file lists have to describe that payload, which nothing above
# asks. Everything before this compares the checkout and the components against
# each other; the spec that ships them is checked only by the greps written out
# by hand in this file. That is how LineXinBar's own spec came to package a
# drawing that had left the tree, and to leave forty-five installed files in no
# package at all — neither visible until rpmbuild reached the end of a build it
# had already paid for in full.
#
# Both directions, because RPM fails on both: an entry with nothing behind it
# is "File not found", and a staged file no entry covers is "Installed (but
# unpackaged) file(s) found".
package_note "checking the Fedora file lists against the staged payload"
spec_root="$stage"
spec_lists="$(mktemp -d)"
spec_entries() {
    awk '
        /^%files/ { inside = 1; next }
        /^%(changelog|prep|build|check|install|package|description|pre|post|preun|postun)/ { inside = 0 }
        !inside { next }
        /^[[:space:]]*(#|$)/ { next }
        # %license and %doc are filled by RPM from the source tree, not from
        # the buildroot, so they are not part of what install.sh stages.
        /^%(license|doc)[[:space:]]/ { next }
        {
            entry = $0
            kind = "path"
            if (entry ~ /^%dir[[:space:]]/) { kind = "dir"; sub(/^%dir[[:space:]]+/, "", entry) }
            sub(/^%config\([^)]*\)[[:space:]]+/, "", entry)
            sub(/^%config[[:space:]]+/, "", entry)
            print kind "\t" entry
        }
    ' "$PACKAGING_DIR/fedora/distribumpy.spec"
}

: > "$spec_lists/packaged"
while IFS=$'\t' read -r kind entry; do
    entry="$(printf '%s\n' "$entry" | sed \
        -e 's|%{_bindir}|/usr/bin|g' \
        -e 's|%{_datadir}|/usr/share|g' \
        -e 's|%{_metainfodir}|/usr/share/metainfo|g' \
        -e 's|%{_prefix}|/usr|g')"
    # Refused rather than skipped: an entry this cannot read is an entry that
    # would go unchecked, which is the state the whole check exists to end.
    if [[ "$entry" == *'%{'* ]]; then
        package_die "check.sh cannot expand the %files entry $entry.
Teach the expansions above the macro rather than leaving the entry unchecked."
    fi
    entry="${entry%/}"
    if [[ "$kind" == dir ]]; then
        # %dir packages the directory itself and none of its contents, so it
        # covers nothing: a file under it still needs an entry of its own.
        [[ -d "$spec_root$entry" ]] \
            || package_die "the spec packages the directory $entry, which nothing creates"
        continue
    fi
    if [[ -d "$spec_root$entry" ]]; then
        (cd "$spec_root" && find ".$entry" \( -type f -o -type l \) -printf '%p\n') \
            | sed 's|^\./||' >> "$spec_lists/packaged"
    elif [[ -f "$spec_root$entry" || -L "$spec_root$entry" ]]; then
        printf '%s\n' "${entry#/}" >> "$spec_lists/packaged"
    else
        package_die "the spec packages $entry, which nothing installs"
    fi
done < <(spec_entries)

sort -u "$spec_lists/packaged" -o "$spec_lists/packaged"
(cd "$spec_root" && find . -mindepth 1 \( -type f -o -type l \) -printf '%P\n' | sort) \
    > "$spec_lists/staged"
if comm -23 "$spec_lists/staged" "$spec_lists/packaged" | grep -q .; then
    package_die "installed and packaged by no %files section: $(
        comm -23 "$spec_lists/staged" "$spec_lists/packaged" | tr '\n' ' ')"
fi
rm -rf -- "$spec_lists"

package_note "all checks passed for distribumpy $PACKAGE_VERSION"
