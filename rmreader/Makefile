.PHONY: test build update-goldens fmt fmt-check clippy hooks
test:
	nix develop -c cargo test
build:
	nix build
update-goldens:
	nix develop -c env RMREADER_UPDATE_GOLDENS=1 cargo test --test visual
fmt:
	nix develop -c cargo fmt
fmt-check:
	nix develop -c cargo fmt --check
clippy:
	nix develop -c cargo clippy --all-targets -- -D warnings
# Enable the tracked git hooks (once per clone / machine).
hooks:
	git config core.hooksPath .githooks
	@echo "pre-commit hook enabled: cargo fmt --check"
