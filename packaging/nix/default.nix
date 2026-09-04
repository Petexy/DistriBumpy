# `nix-build packaging/nix`, for somebody who is not using flakes.
#
# The toolkit is not in nixpkgs, so it has to be fetched: by default from its
# own repository, and from wherever this is pointed instead. A local checkout
# is the useful form of that while both are being worked on:
#
#   nix-build packaging/nix --arg lxb-toolkit-src ~/GitHub/lxb-toolkit
{
  pkgs ? import <nixpkgs> { },
  lxb-toolkit-src ? builtins.fetchGit {
    url = "https://github.com/Petexy/lxb-toolkit";
    ref = "main";
  },
}:

let
  lxb-toolkit = pkgs.callPackage "${lxb-toolkit-src}/packaging/nix/package.nix" {
    src = lxb-toolkit-src;
  };
in
pkgs.callPackage ./package.nix {
  inherit lxb-toolkit;
  src = ../..;
}
