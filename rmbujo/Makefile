.PHONY: test build update-goldens fmt clippy
test:
	nix develop -c cargo test
build:
	nix build
update-goldens:
	nix develop -c env RMBUJO_UPDATE_GOLDENS=1 cargo test --test visual
fmt:
	nix develop -c cargo fmt
clippy:
	nix develop -c cargo clippy -- -D warnings
