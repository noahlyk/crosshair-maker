# Maintainer: noahlyk <noahlykins@gmail.com>
pkgname=crosshair-maker
pkgver=0.10.0
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

# Local builds from a git checkout report the tag, commit count and hash, the same as `make`:
#   0.10.0+2.g849d873, or 0.10.0+2.g849d873.dirty with uncommitted changes.
# Anywhere without git (or on the release tag) the version above is used.
pkgver() {
    cd "$startdir"
    git rev-parse --git-dir >/dev/null 2>&1 || { echo "$pkgver"; return; }

    local version meta=() described dirty
    if described=$(git describe --tags --long --match 'v[0-9]*' 2>/dev/null); then
        local rest="${described%-g*}"
        version="${rest%-*}"
        version="${version#v}"
        local count="${rest##*-}"
        (( count > 0 )) && meta+=("$count.g${described##*-g}")
    else
        version="0.0.0"
        meta+=("g$(git rev-parse --short HEAD)")
    fi
    [ -n "$(git status --porcelain --untracked-files=no)" ] && meta+=(dirty)

    if (( ${#meta[@]} )); then
        local IFS=.
        version+="+${meta[*]}"
    fi
    echo "$version"
}
sha256sums=()

build() {
    cd "$startdir"
    CROSSHAIR_MAKER_VERSION="$pkgver" cargo build --release --locked
}

package() {
    cd "$startdir"
    install -Dm755 "target/release/crosshair-maker" "$pkgdir/usr/bin/crosshair-maker"
    install -Dm644 "assets/crosshair-maker.desktop" "$pkgdir/usr/share/applications/crosshair-maker.desktop"
    install -Dm644 "assets/crosshair-maker.png" "$pkgdir/usr/share/icons/hicolor/256x256/apps/crosshair-maker.png"
    install -Dm644 "LICENSE" "$pkgdir/usr/share/licenses/crosshair-maker/LICENSE"
}
