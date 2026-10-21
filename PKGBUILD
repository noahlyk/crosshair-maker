# Maintainer: noahlyk <noahlykins@gmail.com>
pkgname=crosshair-maker
pkgver=0.7.3
pkgrel=1
pkgdesc="Crosshair overlay creator with SVG rendering and preview"
arch=('x86_64' 'aarch64')
url="https://github.com/noahlyk/crosshair-maker"
license=('GPL-3.0-only')
depends=('libx11' 'libxcb' 'wayland' 'libxkbcommon' 'vulkan-icd-loader')
makedepends=('rust' 'cargo')
options=('!debug')
install=crosshair-maker.install

source=()
sha256sums=()

build() {
    cd "$startdir"
    cargo build --release --locked
}

package() {
    cd "$startdir"
    install -Dm755 "target/release/crosshair-maker" "$pkgdir/usr/bin/crosshair-maker"
    install -Dm644 "assets/crosshair-maker.desktop" "$pkgdir/usr/share/applications/crosshair-maker.desktop"
    install -Dm644 "assets/crosshair-maker.png" "$pkgdir/usr/share/icons/hicolor/256x256/apps/crosshair-maker.png"
    install -Dm644 "LICENSE" "$pkgdir/usr/share/licenses/crosshair-maker/LICENSE"
}
