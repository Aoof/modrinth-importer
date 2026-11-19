#!/bin/sh
# Modrinth Importer installer for macOS and Linux.
#
#   curl -fsSL https://raw.githubusercontent.com/Aoof/modrinth-importer/master/install/install.sh | sh
#
# Options (pass after `sh -s --` when piping):
#   --cli-only        install only the `mrimport` command
#   --gui-only        install only the desktop app
#   --version X.Y.Z   install a specific release instead of the latest
#   --system          install the CLI to /usr/local/bin (uses sudo if needed)
#   --uninstall       remove everything this script installed
set -eu

REPO="Aoof/modrinth-importer"
WANT_CLI=1
WANT_GUI=1
VERSION=""
SYSTEM=0
UNINSTALL=0

while [ $# -gt 0 ]; do
  case "$1" in
    --cli-only) WANT_GUI=0 ;;
    --gui-only) WANT_CLI=0 ;;
    --version) VERSION="${2:?--version needs a value}"; shift ;;
    --system) SYSTEM=1 ;;
    --uninstall) UNINSTALL=1 ;;
    -h|--help) sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "'$1' is required but not installed"; }

OS=$(uname -s)
ARCH=$(uname -m)
case "$ARCH" in
  x86_64|amd64) CPU=x86_64; GUI_ARCH=x64 ;;
  arm64|aarch64) CPU=aarch64; GUI_ARCH=aarch64 ;;
  *) die "unsupported architecture: $ARCH" ;;
esac
case "$OS" in
  Darwin) TARGET="$CPU-apple-darwin"; GUI_ASSET="ModrinthImporter-macos-$GUI_ARCH.dmg" ;;
  Linux)  TARGET="$CPU-unknown-linux-gnu"; GUI_ASSET="ModrinthImporter-linux-$GUI_ARCH.AppImage" ;;
  *) die "unsupported OS: $OS (on Windows use install.ps1)" ;;
esac
CLI_ASSET="mrimport-$TARGET"

if [ "$SYSTEM" -eq 1 ]; then CLI_DIR=/usr/local/bin; else CLI_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"; fi
if [ "$OS" = Darwin ]; then
  if [ -w /Applications ]; then APP_DIR=/Applications; else APP_DIR="$HOME/Applications"; fi
else
  APP_DIR="$HOME/.local/bin"
fi
DESKTOP_FILE="$HOME/.local/share/applications/modrinth-importer.desktop"
ICON_FILE="$HOME/.local/share/icons/modrinth-importer.png"

# Run a command, escalating with sudo only when the CLI directory isn't writable.
can_write() { d=$1; while [ ! -e "$d" ]; do d=$(dirname "$d"); done; [ -w "$d" ]; }
run_cli_dir() { if can_write "$CLI_DIR"; then "$@"; else sudo "$@"; fi; }

if [ "$UNINSTALL" -eq 1 ]; then
  run_cli_dir rm -f "$CLI_DIR/mrimport"
  if [ "$OS" = Darwin ]; then
    rm -rf "/Applications/Modrinth Importer.app" "$HOME/Applications/Modrinth Importer.app" 2>/dev/null || sudo rm -rf "/Applications/Modrinth Importer.app"
  else
    rm -f "$APP_DIR/ModrinthImporter.AppImage" "$DESKTOP_FILE" "$ICON_FILE"
  fi
  say "Modrinth Importer removed."
  exit 0
fi

need curl
if command -v sha256sum >/dev/null 2>&1; then SHA="sha256sum"; else need shasum; SHA="shasum -a 256"; fi

if [ -n "$VERSION" ]; then
  BASE="https://github.com/$REPO/releases/download/v${VERSION#v}"
else
  BASE="https://github.com/$REPO/releases/latest/download"
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT INT TERM

say "Fetching checksums..."
curl -fsSL "$BASE/SHA256SUMS" -o "$TMP/SHA256SUMS" || die "could not download release metadata (does the release exist?)"

fetch() { # fetch <asset>: download + verify into $TMP/<asset>
  say "Downloading $1..."
  curl -fSL --progress-bar "$BASE/$1" -o "$TMP/$1" || die "download failed: $1"
  want=$(grep " \*\{0,1\}$1\$" "$TMP/SHA256SUMS" | awk '{print $1}' | head -n1)
  [ -n "$want" ] || die "$1 is not listed in SHA256SUMS"
  got=$($SHA "$TMP/$1" | awk '{print $1}')
  [ "$want" = "$got" ] || die "checksum mismatch for $1"
}

if [ "$WANT_CLI" -eq 1 ]; then
  fetch "$CLI_ASSET"
  chmod +x "$TMP/$CLI_ASSET"
  run_cli_dir mkdir -p "$CLI_DIR"
  run_cli_dir install -m 755 "$TMP/$CLI_ASSET" "$CLI_DIR/mrimport"
  say "Installed CLI: $CLI_DIR/mrimport"
  case ":$PATH:" in
    *":$CLI_DIR:"*) ;;
    *) say "Note: $CLI_DIR is not on your PATH. Add this to your shell profile (e.g. ~/.zshrc):"
       say "  export PATH=\"$CLI_DIR:\$PATH\"" ;;
  esac
fi

if [ "$WANT_GUI" -eq 1 ]; then
  fetch "$GUI_ASSET"
  if [ "$OS" = Darwin ]; then
    mkdir -p "$APP_DIR"
    MNT="$TMP/mnt"; mkdir -p "$MNT"
    hdiutil attach -nobrowse -quiet -mountpoint "$MNT" "$TMP/$GUI_ASSET"
    rm -rf "$APP_DIR/Modrinth Importer.app"
    cp -R "$MNT/Modrinth Importer.app" "$APP_DIR/"
    hdiutil detach -quiet "$MNT"
    xattr -dr com.apple.quarantine "$APP_DIR/Modrinth Importer.app" 2>/dev/null || true
    # Developer build: seal the bundle with a local ad-hoc signature so Gatekeeper
    # doesn't report it as damaged (no Apple Developer ID is involved).
    if command -v codesign >/dev/null 2>&1; then
      if codesign --force --deep --sign - "$APP_DIR/Modrinth Importer.app" >/dev/null 2>&1; then
        say "Signed app locally (ad-hoc)."
      else
        say "Note: could not sign the app; if macOS reports it as damaged, run:"
        say "  xattr -dr com.apple.quarantine \"$APP_DIR/Modrinth Importer.app\""
      fi
    fi
    say "Installed app: $APP_DIR/Modrinth Importer.app"
  else
    mkdir -p "$APP_DIR" "$(dirname "$DESKTOP_FILE")" "$(dirname "$ICON_FILE")"
    install -m 755 "$TMP/$GUI_ASSET" "$APP_DIR/ModrinthImporter.AppImage"
    curl -fsSL "https://raw.githubusercontent.com/$REPO/master/src-tauri/icons/128x128.png" -o "$ICON_FILE" || true
    cat > "$DESKTOP_FILE" <<DESK
[Desktop Entry]
Type=Application
Name=Modrinth Importer
Comment=Import mods and modpacks to Modrinth
Exec=$APP_DIR/ModrinthImporter.AppImage
Icon=$ICON_FILE
Categories=Utility;Game;
Terminal=false
DESK
    say "Installed app: $APP_DIR/ModrinthImporter.AppImage"
  fi
fi

say "Done."
