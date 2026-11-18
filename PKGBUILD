# Maintainer: Arch Control Center
pkgname=arch-control-center
pkgver=0.2.0
pkgrel=1
pkgdesc="Hyprland shell: dock, launcher, settings, and a 3D control center"
arch=('x86_64')
url="https://github.com/levks787-star/cyber_Ui_Arch"
license=('MIT')
depends=('mesa' 'libxkbcommon' 'wayland' 'alsa-lib' 'vulkan-icd-loader')
makedepends=('rustup' 'pkgconf' 'base-devel')
source=()
sha256sums=()

build() {
    cd "$startdir"
    rustup default stable
    cargo build --release --locked
}

package() {
    cd "$startdir"
    install -Dm755 target/release/arch-control-center "$pkgdir/usr/bin/arch-control-center"
    install -Dm644 assets/arch-control-center.desktop "$pkgdir/usr/share/applications/arch-control-center.desktop"
    install -Dm644 assets/arch-shell.desktop "$pkgdir/usr/share/applications/arch-shell.desktop"
    install -Dm644 hypr/arch-control-center.conf "$pkgdir/usr/share/arch-control-center/hyprland.conf"
    install -Dm755 install.sh "$pkgdir/usr/share/arch-control-center/install.sh"
}
