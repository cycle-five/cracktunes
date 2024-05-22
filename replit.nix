{ pkgs }: {
  deps = [
    pkgs.postgresql
    pkgs.rustup
    pkgs.sqlx-cli
    pkgs.rustfmt
    pkgs.cargo
    pkgs.rust-analyzer
    pkgs.pkg-config
  ];
}
