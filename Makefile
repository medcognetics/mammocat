.PHONY: quality style test

quality:
	cargo fmt -- --check
	cargo check --all-features
	cargo clippy --all-features -- -D warnings

style:
	cargo fix --allow-dirty --all-features
	cargo clippy --all-features --fix --allow-dirty
	cargo fmt

test: 
	cargo test
