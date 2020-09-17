.PHONY: init
init:
	rm -rf .git/hooks
	ln -s ../scripts/git-hooks .git/hooks
	chmod -R +x ./scripts/*

.PHONY: update-version
update-version:
	sed 's/version = "0.0.0"/version = "$(VERSION)"/g' Cargo.toml > Cargo.toml.tmp
	mv Cargo.toml.tmp Cargo.toml

.PHONY: clean
clean:
	cargo clean

.PHONY: build
build:
	cargo build --all-targets

.PHONY: release
release:
	cargo build --release

.PHONY: test
test:
	cargo test

.PHONY: scan
scan:
	# cargo clippy --all-targets --all-features -- -D warnings
	cargo fmt --all -- --check
