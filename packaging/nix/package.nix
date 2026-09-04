{
  lib,
  rustPlatform,
  pkg-config,
  makeWrapper,
  flatpak,
  glib,
  wayland,
  libxkbcommon,
  libGL,
  vulkan-loader,
  # The design language, as a derivation. It is a *build* dependency and not a
  # runtime one: `lxb-app` is a Rust path dependency, so cargo compiles those
  # sources into this binary and nothing of the toolkit is referenced once it
  # is built. It is here rather than in nixpkgs because that is where it is —
  # the flake at the root of this checkout is what supplies it.
  lxb-toolkit,
  src ? ../..,
}:

let
  sourceRoot = toString src;
  cleanSrc = lib.cleanSourceWith {
    inherit src;
    filter =
      path: type:
      let
        relative = lib.removePrefix "${sourceRoot}/" (toString path);
      in
      !(
        relative == ".git"
        || lib.hasPrefix ".git/" relative
        || relative == "target"
        || lib.hasPrefix "target/" relative
        || relative == "packaging/out"
        || lib.hasPrefix "packaging/out/" relative
        || relative == "result"
        || lib.hasPrefix "result-" relative
      );
  };
  version = lib.removeSuffix "\n" (builtins.readFile ../../VERSION);

  # Opened by name at run time rather than linked, so nothing that reads the
  # executable can find them and the usual RPATH machinery never sees them
  # either. wayland is in the list twice over — it is linked as well — and it
  # costs nothing to say so once here.
  openedAtRuntime = [
    wayland
    libxkbcommon
    libGL
    vulkan-loader
  ];
in
rustPlatform.buildRustPackage {
  pname = "distribumpy";
  inherit version;
  src = cleanSrc;

  cargoLock.lockFile = "${cleanSrc}/Cargo.lock";

  strictDeps = true;
  nativeBuildInputs = [ pkg-config makeWrapper ];
  buildInputs = [ flatpak glib ] ++ openedAtRuntime;

  # Cargo.toml names the toolkit's crates at /usr/share, which is where every
  # other distribution here puts them and is nowhere at all under Nix. This is
  # the one line that makes the FHS assumption a store path; the lock file is
  # untouched by it, because a path dependency carries no source there.
  postPatch = ''
    substituteInPlace Cargo.toml \
      --replace-fail "/usr/share/lxb-toolkit/crates" \
                     "${lxb-toolkit}/share/lxb-toolkit/crates"
  '';

  # cargoInstallHook would install the binary and nothing else — no desktop
  # entry, no icon, no AppStream data — and a store that does not appear in the
  # menu is a store nobody opens. install.sh is what every other package
  # definition here uses, and using it means the Nix build cannot quietly ship
  # a different set of files than the .deb does.
  installPhase = ''
    runHook preInstall

    # install.sh reads the release directory of a target dir; buildRustPackage
    # builds under a target triple, so point it at the parent of that.
    targetDir="target"
    if [ ! -d "target/release" ]; then
      targetDir="$(dirname "$(dirname "$(readlink -f target/*/release)")")"
    fi

    bash packaging/install.sh \
      --destdir "$out" \
      --prefix "" \
      --target-dir "$targetDir"

    install -Dm0644 LICENSE "$out/share/licenses/distribumpy/LICENSE"
    install -Dm0644 README.md "$out/share/doc/distribumpy/README.md"

    runHook postInstall
  '';

  postFixup = ''
    wrapProgram "$out/bin/distribumpy" \
      --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath openedAtRuntime}"
  '';

  meta = {
    description = "A Flatpak store in the LineXinBar design language, shown as Software Hub";
    homepage = "https://github.com/Petexy/distribumpy";
    license = lib.licenses.gpl3Only;
    platforms = lib.platforms.linux;
    mainProgram = "distribumpy";
  };
}
