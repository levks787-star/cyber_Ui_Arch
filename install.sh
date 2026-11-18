#!/usr/bin/env bash
# Install Arch Control Center without rewriting an existing Hyprland config.
# A timestamped backup is made only when one source= line is appended.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="$PREFIX/bin"
APP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
HYPR_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/hypr"
SNIPPET_NAME="arch-control-center.conf"
SNIPPET_DST="$HYPR_DIR/$SNIPPET_NAME"
CONF="$HYPR_DIR/hyprland.conf"
SOURCE_LINE="source = ~/.config/hypr/$SNIPPET_NAME"

uninstall() {
    rm -f "$BIN_DIR/arch-control-center"
    rm -f "$APP_DIR/arch-control-center.desktop" "$APP_DIR/arch-shell.desktop"
    if [[ -f "$CONF" ]]; then
        tmp="$(mktemp)"
        grep -v -F "$SOURCE_LINE" "$CONF" | grep -v -F "source = $SNIPPET_DST" >"$tmp" || true
        mv "$tmp" "$CONF"
    fi
    rm -f "$SNIPPET_DST"
    echo "Снято. Резервные копии hyprland.conf.bak.* не трогались."
}

if [[ "${1:-}" == "--uninstall" ]]; then
    uninstall
    exit 0
fi

if command -v pacman >/dev/null 2>&1; then
    sudo pacman -S --needed base-devel git rustup pkgconf mesa libxkbcommon wayland alsa-lib vulkan-icd-loader
fi

if ! command -v rustup >/dev/null 2>&1 && ! command -v cargo >/dev/null 2>&1; then
    echo "Нужен rustup или cargo." >&2
    exit 1
fi
if command -v rustup >/dev/null 2>&1; then
    rustup default stable >/dev/null
fi

(cd "$ROOT" && cargo build --release)

install -Dm755 "$ROOT/target/release/arch-control-center" "$BIN_DIR/arch-control-center"
install -Dm644 "$ROOT/assets/arch-control-center.desktop" "$APP_DIR/arch-control-center.desktop"
install -Dm644 "$ROOT/assets/arch-shell.desktop" "$APP_DIR/arch-shell.desktop"
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$APP_DIR" || true
fi

mkdir -p "$HYPR_DIR"
install -Dm644 "$ROOT/hypr/arch-control-center.conf" "$SNIPPET_DST"

if [[ -f "$CONF" ]]; then
    if grep -q "arch-control-center.conf" "$CONF"; then
        echo "hyprland.conf уже подключает $SNIPPET_NAME, файл не менялся."
    else
        stamp="$(date +%Y%m%d%H%M%S)"
        cp "$CONF" "$CONF.bak.$stamp"
        printf '\n# Arch Control Center shell\n%s\n' "$SOURCE_LINE" >>"$CONF"
        echo "Добавлена одна строка source. Копия: $CONF.bak.$stamp"
    fi
else
    echo "hyprland.conf нет, сниппет лежит в $SNIPPET_DST и не был подключён сам."
fi

echo "Готово: $BIN_DIR/arch-control-center --shell"
echo "Снять: $ROOT/install.sh --uninstall"
