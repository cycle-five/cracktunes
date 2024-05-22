# configuration.nix
{ pkgs, ... }: {
  nixpkgs.overlays = [
    (import "${fetchTarball "https://github.com/nix-community/fenix/archive/main.tar.gz"}/overlay.nix")
  ];
  
  deps = [
    pkgs.cmake
  ];environment.systemPackages = with pkgs; [
    (fenix.complete.withComponents [
      "cargo"
      "clippy"
      "rust-src"
      "rustc"
      "rustfmt"
      "cmake"
    ])
    rust-analyzer-nightly
  ];
}