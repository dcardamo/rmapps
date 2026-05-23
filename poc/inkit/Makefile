.PHONY: test build fmt fmt-check clippy hooks
test:
	nix develop -c cargo test
build:
	nix develop -c cargo build
fmt:
	nix develop -c cargo fmt
fmt-check:
	nix develop -c cargo fmt --check
clippy:
	nix develop -c cargo clippy --all-targets -- -D warnings
hooks:
	git config core.hooksPath .githooks
	@echo "pre-commit hook enabled: cargo fmt --check"
