#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=packaging/lib.sh
source "$script_dir/../lib.sh"

output_dir="$PACKAGING_DIR/out/debian"
allow_foreign=false

usage() {
    cat <<'EOF'
Usage: packaging/debian/build.sh [--output-dir DIR] [--allow-foreign-host]

Builds the Debian binary package from the current working tree. A deployable
package must be built on Debian (or a Debian derivative) so its ABI and
generated shared-library dependencies match the target system.

--allow-foreign-host is for looking at the package's shape on another
distribution. It is not a way to produce something installable there.
EOF
}

while (($#)); do
    case "$1" in
        --output-dir)
            (($# >= 2)) || package_die "--output-dir requires a value"
            output_dir="$2"
            shift 2
            ;;
        --allow-foreign-host)
            allow_foreign=true
            shift
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *) package_die "unknown Debian builder option: $1" ;;
    esac
done

if [[ "$allow_foreign" != true ]] && ! host_is_like debian; then
    package_die "build Debian packages on Debian/Ubuntu; use --allow-foreign-host only for metadata testing"
fi
if [[ "$output_dir" != /* ]]; then
    output_dir="$PWD/$output_dir"
fi

# Everything the build needs is checked here, before anything is compiled, and
# all of it at once. A bare Debian container has none of it, and meeting the
# gaps one per attempt — the packaging tools, then Rust, then each library a
# minute into a release build — is how a first build takes an afternoon. Each
# gap is named by the Debian package that fills it, so the refusal is one apt
# line.
#
# The libraries are the ones this build links: probed through pkg-config by a
# -sys crate, or named by a #[link] attribute. What the program only opens at
# run time is not a build's business, and is not refused over.
#
# Rust is rustup rather than Debian's cargo package, which is 1.85 on Debian 13
# and older than the locked dependency graph allows. rustup's proxies go in
# /usr/bin, and in a distrobox or toolbox they find the toolchain already in
# the shared ~/.rustup.
missing_packages=()
need() {
    local kind="$1" name="$2" package="$3"
    case "$kind" in
        command) command -v "$name" >/dev/null 2>&1 && return ;;
        library) pkg-config --exists "$name" 2>/dev/null && return ;;
    esac
    [[ " ${missing_packages[*]} " == *" $package "* ]] || missing_packages+=("$package")
}
need command dpkg-deb dpkg
need command dpkg-shlibdeps dpkg-dev
need command md5sum coreutils
need command cc build-essential
need command pkg-config pkg-config
need command cargo rustup
need command rustc rustup
need library alsa libasound2-dev
need library flatpak libflatpak-dev
need library gio-2.0 libglib2.0-dev
need library glib-2.0 libglib2.0-dev
need library gobject-2.0 libglib2.0-dev
need library libudev libudev-dev
need library xkbcommon libxkbcommon-dev
if ((${#missing_packages[@]})); then
    package_die "the build prerequisites are not all installed; install them with:
  sudo apt install ${missing_packages[*]}"
fi
require_rust_version 1.90 \
    "Debian's cargo package is older than that; install rustup in its place: sudo apt install rustup && rustup default stable"
require_toolkit_sources

# Its own target directory rather than the checkout's target/. A Debian package
# is often built in a container that shares the checkout with a host running
# another distribution, and sharing target/ would put binaries linked against
# Debian's library versions where the host's own build left its binaries.
target_dir="${CARGO_TARGET_DIR:-$PROJECT_ROOT/target/debian}"
if [[ "$target_dir" != /* ]]; then
    target_dir="$PROJECT_ROOT/$target_dir"
fi
package_note "building the release artefact"
RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$PROJECT_ROOT=/usr/src/distribumpy-$PACKAGE_VERSION" \
    CARGO_TARGET_DIR="$target_dir" \
    cargo build --manifest-path "$PROJECT_ROOT/Cargo.toml" --release --locked

work="$(package_work_dir distribumpy-debian)"
cleanup() {
    if [[ -n "${work:-}" && "$work" == */distribumpy-debian.* && -d "$work" ]]; then
        rm -rf -- "$work"
    fi
}
trap cleanup EXIT

architecture="$(dpkg --print-architecture)"
name=distribumpy
package_root="$work/$name"

mkdir -p "$output_dir"
# dpkg-shlibdeps insists on being run from a directory with a debian/control in
# it, whether or not this build has anything else debhelper-shaped about it.
mkdir -p "$work/shlibs/debian"
sed -e "s/@TOOLKIT_VERSION@/$(package_toolkit_requirement)/" \
    "$script_dir/source-control.in" > "$work/shlibs/debian/control"

"$PACKAGING_DIR/install.sh" \
    --destdir "$package_root" \
    --prefix /usr \
    --target-dir "$target_dir" >/dev/null

# No /usr/share/licenses here: that is the RPM and Arch convention. On Debian
# the copyright file is the licence record, and it points at the GPL-3 text
# every Debian system already carries in /usr/share/common-licenses.
install -Dm0644 "$script_dir/copyright" "$package_root/usr/share/doc/$name/copyright"
# Staged before the package is built, so it is weighed by Installed-Size and
# listed in md5sums.
install -Dm0644 "$PROJECT_ROOT/README.md" "$package_root/usr/share/doc/$name/README.md"

if command -v strip >/dev/null 2>&1; then
    strip --strip-unneeded "$package_root/usr/bin/distribumpy"
fi

shlib_depends=""
shlib_arguments=(-e"$package_root/usr/bin/distribumpy")
# On a foreign host there is no dpkg database mapping libc and libflatpak to
# Debian packages, so dpkg-shlibdeps fails outright and the flag that exists
# for structure testing cannot do any. Downgrade that to a warning there and
# nowhere else: on Debian the strict form is the whole point, because a missed
# library is a package that installs and then does not run.
if [[ "$allow_foreign" == true ]]; then
    shlib_arguments+=(--ignore-missing-info)
fi
shlib_output="$({
    cd "$work/shlibs"
    dpkg-shlibdeps -O "${shlib_arguments[@]}"
})"
if [[ "$shlib_output" == shlibs:Depends=* ]]; then
    shlib_depends="${shlib_output#shlibs:Depends=}"
elif [[ "$allow_foreign" == true && -z "$shlib_output" ]]; then
    # Everything was ignored above, so there is nothing left to name. The
    # package's shape is still worth looking at; what it declares is not, and
    # must not be mistaken for a package that could ship.
    package_note "warning: $name has no shared-library Depends — this host cannot resolve them"
else
    package_die "could not determine Debian shared-library dependencies for $name"
fi

installed_size="$(du -sk "$package_root" | awk '{print $1}')"
mkdir -p "$package_root/DEBIAN"
awk \
    -v version="$PACKAGE_VERSION" \
    -v architecture="$architecture" \
    -v dependencies="$shlib_depends" \
    -v installed_size="$installed_size" \
    '{
        gsub(/@VERSION@/, version)
        gsub(/@ARCH@/, architecture)
        gsub(/@SHLIB_DEPENDS@/, dependencies)
        gsub(/@INSTALLED_SIZE@/, installed_size)
        print
    }' "$script_dir/control.in" > "$package_root/DEBIAN/control"
# With nothing to link against the substitution leaves a leading comma, which
# is a field dpkg rejects rather than ignores.
sed -i -e 's/^Depends: , /Depends: /' "$package_root/DEBIAN/control"

(
    cd "$package_root"
    find usr -type f -print0 | sort -z | xargs -0 md5sum
) > "$package_root/DEBIAN/md5sums"

artifact="$output_dir/${name}_${PACKAGE_VERSION}-1_${architecture}.deb"
dpkg-deb --root-owner-group -Zxz --build "$package_root" "$artifact"
dpkg-deb --info "$artifact" >/dev/null
package_note "created $artifact"
