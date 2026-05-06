BINARY   := tome
INSTALL  := $(HOME)/.cargo/bin/$(BINARY)

.PHONY: build install clean

build:
	cargo build --release

install: build
	cp -f target/release/$(BINARY) $(INSTALL)
	# Ad-hoc sign so macOS Gatekeeper does not kill the binary
	codesign --sign - --force $(INSTALL)
	@echo "Installed and signed $(INSTALL)"

clean:
	cargo clean
