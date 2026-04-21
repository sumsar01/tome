.PHONY: build install release clean

build:
	cargo build --release

# Canonical install — always lands at ~/.cargo/bin/tome (the path MCP config expects)
install:
	cargo install --path .

# Build release binary, create GitHub release, and install
release:
	@VERSION=$$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/'); \
	echo "Releasing v$$VERSION..."; \
	cargo build --release && \
	gh release create v$$VERSION target/release/tome \
		--title "v$$VERSION" \
		--generate-notes && \
	cargo install --path .

clean:
	cargo clean
