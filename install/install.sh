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
#   --uninstall       remove the CLI, the app, settings, caches and stored credentials
#   --keep-data       with --uninstall: keep settings and credentials
set -eu

REPO="Aoof/modrinth-importer"
WANT_CLI=1
WANT_GUI=1
VERSION=""
SYSTEM=0
UNINSTALL=0
KEEP_DATA=0

while [ $# -gt 0 ]; do
  case "$1" in
    --cli-only) WANT_GUI=0 ;;
    --gui-only) WANT_CLI=0 ;;
    --version) VERSION="${2:?--version needs a value}"; shift ;;
    --system) SYSTEM=1 ;;
    --uninstall) UNINSTALL=1 ;;
    --keep-data) KEEP_DATA=1 ;;
    -h|--help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
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
# Advanced: install/remove the desktop app somewhere else.
[ -z "${MRIMPORT_APPS_DIR:-}" ] || APP_DIR="$MRIMPORT_APPS_DIR"
DESKTOP_FILE="$HOME/.local/share/applications/modrinth-importer.desktop"
ICON_FILE="$HOME/.local/share/icons/modrinth-importer.png"

# Run a command, escalating with sudo only when the CLI directory isn't writable.
can_write() { d=$1; while [ ! -e "$d" ]; do d=$(dirname "$d"); done; [ -w "$d" ]; }
run_cli_dir() { if can_write "$CLI_DIR"; then "$@"; else sudo "$@"; fi; }

if [ "$UNINSTALL" -eq 1 ]; then
  REMOVED=0
  gone() { say "  removed $1"; REMOVED=$((REMOVED + 1)); }
  rm_any() { # remove a path, using sudo only if its folder isn't writable
    [ -e "$1" ] || return 0
    if [ -w "$(dirname "$1")" ]; then rm -rf "$1"; else sudo rm -rf "$1"; fi
    gone "$1"
  }

  # Settings, caches, webview data and keychain credentials. The CLI knows exactly
  # what it created; without it, fall back to the same list built in here.
  purge_fallback() {
    ID=com.aoof.modrinthimporter
    if [ "$OS" = Darwin ]; then
      for p in \
        "$HOME/Library/Application Support/$ID" "$HOME/Library/Application Support/com.Aoof.ModrinthImporter" \
        "$HOME/Library/Caches/$ID" "$HOME/Library/Caches/com.Aoof.ModrinthImporter" \
        "$HOME/Library/WebKit/$ID" "$HOME/Library/HTTPStorages/$ID" "$HOME/Library/Logs/$ID" \
        "$HOME/Library/Preferences/$ID.plist" "$HOME/Library/Saved Application State/$ID.savedState"; do
        rm_any "$p"
      done
      if command -v security >/dev/null 2>&1; then
        for a in modrinth-token curseforge-api-key; do
          if security delete-generic-password -s modrinth-importer -a "$a" >/dev/null 2>&1; then gone "keychain item $a"; fi
        done
      fi
    else
      for base in "${XDG_DATA_HOME:-$HOME/.local/share}" "${XDG_CACHE_HOME:-$HOME/.cache}" "${XDG_CONFIG_HOME:-$HOME/.config}"; do
        rm_any "$base/$ID"
        rm_any "$base/modrinthimporter"
      done
      if command -v secret-tool >/dev/null 2>&1; then
        for a in modrinth-token curseforge-api-key; do
          if secret-tool clear service modrinth-importer username "$a" >/dev/null 2>&1; then gone "keyring item $a"; fi
        done
      fi
    fi
  }

  if [ "$KEEP_DATA" -eq 0 ]; then
    CLI_BIN=""
    for c in "$CLI_DIR/mrimport" "$HOME/.local/bin/mrimport" /usr/local/bin/mrimport; do
      if [ -x "$c" ]; then CLI_BIN=$c; break; fi
    done
    if [ -n "$CLI_BIN" ] && "$CLI_BIN" purge --yes; then
      :
    else
      purge_fallback
    fi
  fi

  # Program files.
  for d in "$CLI_DIR" "$HOME/.local/bin" /usr/local/bin; do
    rm_any "$d/mrimport"
    rm_any "$d/mrimport.new"
  done
  if [ "$OS" = Darwin ]; then
    if [ -n "${MRIMPORT_APPS_DIR:-}" ]; then
      rm_any "$MRIMPORT_APPS_DIR/Modrinth Importer.app"
    else
      rm_any "/Applications/Modrinth Importer.app"
      rm_any "$HOME/Applications/Modrinth Importer.app"
    fi
  else
    rm_any "$APP_DIR/ModrinthImporter.AppImage"
    rm_any "$DESKTOP_FILE"
    rm_any "$ICON_FILE"
    if command -v update-desktop-database >/dev/null 2>&1; then
      update-desktop-database "$(dirname "$DESKTOP_FILE")" >/dev/null 2>&1 || true
    fi
  fi

  if [ "$REMOVED" -eq 0 ]; then say "Nothing to remove."; else say "Modrinth Importer removed ($REMOVED items). Nothing else was installed: no PATH or shell-profile edits to undo."; fi
  exit 0
fi

need curl
if command -v sha256sum >/dev/null 2>&1; then SHA="sha256sum"; else need shasum; SHA="shasum -a 256"; fi

# Releases are published as pre-releases (developer builds), which GitHub's
# "latest" alias skips, so look up the newest release explicitly.
if [ -z "$VERSION" ]; then
  VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases?per_page=1" 2>/dev/null \
    | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"v\{0,1\}\([^"]*\)".*/\1/p' | head -n1)
  [ -n "$VERSION" ] || die "could not determine the latest release (GitHub API unavailable?); try --version X.Y.Z"
fi
BASE="https://github.com/$REPO/releases/download/v${VERSION#v}"

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
