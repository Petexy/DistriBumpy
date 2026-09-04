#!/usr/bin/env bash

# Stage the payload shared by every distro package: the binary and the three
# files that make it an application rather than a command.
#
# This deliberately does not install the licence or any documentation. Where
# those go is the one thing the distributions really disagree about — Arch and
# Fedora want /usr/share/licenses, Debian wants a copyright file under
# /usr/share/doc — so each recipe places its own and this stays the part they
# can all share.

set -euo pipefail

packaging_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=packaging/lib.sh
source "$packaging_dir/lib.sh"

destdir=""
prefix="/usr"
prefix_given=false
target_dir="${CARGO_TARGET_DIR:-$PROJECT_ROOT/target}"
binary=""

usage() {
    cat <<'EOF'
Usage: packaging/install.sh --destdir DIR [--prefix PREFIX]
                            [--target-dir DIR] [--binary FILE]

Stages DistriBumpy into DESTDIR. PREFIX defaults to /usr and --target-dir to
CARGO_TARGET_DIR or ./target; --binary names the executable outright, for a
builder that has put it somewhere neither of those describes.

  bin/distribumpy
  share/applications/distribumpy.desktop
  share/icons/hicolor/scalable/apps/distribumpy.svg
  share/metainfo/io.github.petexy.distribumpy.metainfo.xml

There is no --libdir and no --component. This installs no library, so there is
nothing for a multiarch triplet to disambiguate; and it is one package on every
distribution, because splitting a single executable from its own desktop entry
would produce a package that installs and then cannot be started.
EOF
}

while (($#)); do
    case "$1" in
        --destdir)
            (($# >= 2)) || package_die "--destdir requires a value"
            destdir="$2"
            shift 2
            ;;
        --prefix)
            (($# >= 2)) || package_die "--prefix requires a value"
            prefix="$2"
            prefix_given=true
            shift 2
            ;;
        --target-dir)
            (($# >= 2)) || package_die "--target-dir requires a value"
            target_dir="$2"
            shift 2
            ;;
        --binary)
            (($# >= 2)) || package_die "--binary requires a value"
            binary="$2"
            shift 2
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *) package_die "unknown install option: $1" ;;
    esac
done

# DESTDIR and PREFIX in the environment are what a `make install` takes, and
# this script answered to them before it took flags at all. Both still work, so
# a recipe written against the old shape is not silently installing nothing.
destdir="${destdir:-${DESTDIR:-}}"
# The flag wins where both are given. `--prefix ""` is a real answer — it is
# what the Nix build passes, and a PREFIX left in the environment must not
# quietly turn a store path back into /usr.
if [[ "$prefix_given" != true && -n "${PREFIX:-}" ]]; then
    prefix="$PREFIX"
fi

[[ -n "$destdir" ]] || package_die "--destdir is required"
[[ "$destdir" == /* ]] || package_die "--destdir must be absolute"
[[ -z "$prefix" || "$prefix" == /* ]] || package_die "--prefix must be empty or absolute"

if [[ "$target_dir" != /* ]]; then
    target_dir="$PROJECT_ROOT/$target_dir"
fi
prefix="${prefix%/}"
[[ "$prefix" != "/" ]] || prefix=""
# `--destdir /` is how somebody installs onto the machine they are standing at,
# and it is the one value that would otherwise concatenate into `//usr`.
destdir="${destdir%/}"
install_root="${destdir}${prefix}"

binary="${binary:-${BINARY:-$target_dir/release/distribumpy}}"
[[ -x "$binary" ]] || package_die "missing release binary: $binary
Build it first: cargo build --release"

install -Dm0755 "$binary" "$install_root/bin/distribumpy"
install -Dm0644 "$PROJECT_ROOT/data/distribumpy.desktop" \
    "$install_root/share/applications/distribumpy.desktop"
# Scalable and nothing else. Every mark this draws is a shape evaluated at the
# size it is wanted, so a rasterised copy at six fixed sizes would be the one
# picture of this application that is not made of the same material.
install -Dm0644 "$PROJECT_ROOT/data/distribumpy.svg" \
    "$install_root/share/icons/hicolor/scalable/apps/distribumpy.svg"
# What GNOME Software, Discover and this store itself read to describe an
# application. Shipping it is how DistriBumpy appears in the other two.
install -Dm0644 "$PROJECT_ROOT/data/io.github.petexy.distribumpy.metainfo.xml" \
    "$install_root/share/metainfo/io.github.petexy.distribumpy.metainfo.xml"

package_note "staged distribumpy $PACKAGE_VERSION into $install_root"
