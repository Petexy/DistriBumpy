#!/usr/bin/env bash

# Set the version this project releases under.
#
# `VERSION` at the root of the checkout is that number, and most things read it
# where it stands: the Arch, Debian and Nix definitions, and the source
# archive's name.
#
# Three places cannot read a file and carry the number as a literal instead.
# Cargo's manifest is one: `distribumpy --version` prints CARGO_PKG_VERSION, so
# that manifest is where a running program says what it is. The Fedora spec is
# the second, because `Version:` has to be a literal for the spec to be one
# anyone could submit. The AppStream release list is the third, because it is
# not only a version — it is the entry every other software centre reads to say
# what this release changed, and only a person can write that half.
#
# This writes all of them, which is what makes a release one command rather
# than four edits that have to agree.

set -euo pipefail

scripts_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(cd "$scripts_dir/.." && pwd)"
version_file="$project_root/VERSION"
manifest="$project_root/Cargo.toml"
spec="$project_root/packaging/fedora/distribumpy.spec"
metainfo="$project_root/data/io.github.petexy.distribumpy.metainfo.xml"

die() {
    echo "error: $*" >&2
    exit 1
}

note() {
    echo "==> $*"
}

usage() {
    cat <<'USAGE'
Usage: scripts/bump-version.sh X.Y.Z

Writes the version into VERSION, [package] in Cargo.toml, Cargo.lock,
packaging/fedora/distribumpy.spec and the AppStream release list. Every other
package definition reads VERSION for itself.

Run with the version already in VERSION to write the other four back into
agreement with it.
USAGE
}

case "${1:-}" in
    -h | --help)
        usage
        exit 0
        ;;
    "")
        usage >&2
        exit 1
        ;;
esac

new_version="$1"
shift
[[ $# -eq 0 ]] || die "unexpected argument: $1"

# The same shape packaging/lib.sh insists on when it reads the file back, and
# the same shape a Cargo version and an RPM Version: can both be.
[[ "$new_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] \
    || die "version must be X.Y.Z, not $new_version"

[[ -f "$manifest" ]] || die "no manifest at $manifest"
[[ -f "$spec" ]] || die "no spec at $spec"
[[ -f "$metainfo" ]] || die "no metainfo at $metainfo"

note "VERSION -> $new_version"
printf '%s\n' "$new_version" > "$version_file"

# Only inside [package]. A blind substitution would also rewrite the version
# this asks of lxb-app, which is a requirement on somebody else's release and
# has nothing to do with this one.
note "Cargo.toml [package] -> $new_version"
awk -v version="$new_version" '
    /^\[/ { section = $0 }
    section == "[package]" && /^version[[:space:]]*=/ && !done {
        print "version = \"" version "\""
        done = 1
        next
    }
    { print }
' "$manifest" > "$manifest.tmp"
mv "$manifest.tmp" "$manifest"

note "packaging/fedora/distribumpy.spec Version: -> $new_version"
awk -v version="$new_version" '
    /^Version:[[:space:]]/ && !done {
        printf "Version:        %s\n", version
        done = 1
        next
    }
    { print }
' "$spec" > "$spec.tmp"
mv "$spec.tmp" "$spec"

# A stanza rather than a substitution: AppStream keeps every release, newest
# first, and rewriting the top one would erase the release before this one from
# the history every other software centre shows. If this version is already
# listed — a re-run to put the files back into agreement — leave it alone.
if grep -Fq "<release version=\"$new_version\"" "$metainfo"; then
    note "the metainfo already lists $new_version"
else
    note "data/io.github.petexy.distribumpy.metainfo.xml <- $new_version"
    today="$(date -u +%Y-%m-%d)"
    awk -v version="$new_version" -v today="$today" '
        /<releases>/ && !done {
            print
            printf "    <release version=\"%s\" type=\"development\" date=\"%s\"/>\n", \
                version, today
            done = 1
            next
        }
        { print }
    ' "$metainfo" > "$metainfo.tmp"
    grep -Fq "<release version=\"$new_version\"" "$metainfo.tmp" \
        || die "could not find <releases> in $metainfo"
    mv "$metainfo.tmp" "$metainfo"
fi

# Every packaged build is `--locked` or `--frozen`, so a lock file left behind
# is a build that refuses to start rather than one that quietly updates.
note "Cargo.lock"
(cd "$project_root" && cargo update --workspace --offline >/dev/null 2>&1) \
    || (cd "$project_root" && cargo update --workspace >/dev/null)

note "done. Two things only a person can write:"
echo "    packaging/fedora/distribumpy.spec   %changelog"
echo "    data/io.github.petexy.distribumpy.metainfo.xml   what this release changed"
echo "Then: packaging/build.sh check"
