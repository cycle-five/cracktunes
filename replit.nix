{ pkgs }: {
	deps = [
   		pkgs.vim
   		pkgs.rustup
		pkgs.sqlx-cli
  		pkgs.rustc
		pkgs.rustfmt
		pkgs.cargo
		pkgs.cargo-edit
		pkgs.rust-analyzer
		pkgs.pkg-config
		pkgs.openssl
		pkgs.cmake
	];
}
