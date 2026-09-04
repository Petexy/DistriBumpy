{
  description = "A Flatpak store in the LineXinBar design language, shown as Software Hub";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    lxb-toolkit = {
      url = "github:Petexy/lxb-toolkit";
      # One nixpkgs, so that the toolkit's crates and this program are compiled
      # by one rustc. Two would build, and would be two builds of the same
      # dependency graph for no reason at all.
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, lxb-toolkit, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          distribumpy = pkgs.callPackage ./packaging/nix/package.nix {
            src = ./.;
            lxb-toolkit = lxb-toolkit.packages.${system}.lxb-toolkit;
          };
        in
        {
          inherit distribumpy;
          default = distribumpy;
        });

      apps = forAllSystems (system: rec {
        distribumpy = {
          type = "app";
          program = "${self.packages.${system}.distribumpy}/bin/distribumpy";
        };
        default = distribumpy;
      });
    };
}
