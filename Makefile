# Local builds. The version comes from git (see build.rs):
#   tag v0.10.0 checked out      -> 0.10.0
#   2 commits past v0.10.0       -> 0.10.0+2.g849d873
#   uncommitted changes          -> ...+dirty
# Release builds set CROSSHAIR_MAKER_VERSION explicitly, so they keep the plain release version.

PREFIX ?= /usr/local
BIN := target/release/crosshair-maker

.PHONY: all build install uninstall version clean

all: build

build:
	cargo build --release --locked

install: build
	install -Dm755 $(BIN) $(DESTDIR)$(PREFIX)/bin/crosshair-maker
	install -Dm644 assets/crosshair-maker.desktop $(DESTDIR)$(PREFIX)/share/applications/crosshair-maker.desktop
	install -Dm644 assets/crosshair-maker.png $(DESTDIR)$(PREFIX)/share/icons/hicolor/256x256/apps/crosshair-maker.png
	install -Dm644 LICENSE $(DESTDIR)$(PREFIX)/share/licenses/crosshair-maker/LICENSE

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/crosshair-maker
	rm -f $(DESTDIR)$(PREFIX)/share/applications/crosshair-maker.desktop
	rm -f $(DESTDIR)$(PREFIX)/share/icons/hicolor/256x256/apps/crosshair-maker.png
	rm -f $(DESTDIR)$(PREFIX)/share/licenses/crosshair-maker/LICENSE

version: build
	@$(BIN) --version

clean:
	cargo clean
