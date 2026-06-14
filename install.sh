#!/usr/bin/env sh
set -eu

ROOT_DIR="$(pwd)"
BIN_DIR=${BIN_DIR:-/usr/local/bin}
TARGET="$ROOT_DIR/target/release/mem_012"
DEST="$BIN_DIR/mem012"
CONFIG_PATH=${MEM012_CONFIG:-"$ROOT_DIR/config.toml"}
PROFILE_FILE="$HOME/.bashrc"
EXPORT_LINE="export MEM012_CONFIG=\"$CONFIG_PATH\""

if [ -f "$HOME/.zshrc" ]; then
    PROFILE_FILE="$HOME/.zshrc"
fi

cargo build --release --manifest-path "$ROOT_DIR/Cargo.toml"

if [ -d "$BIN_DIR" ] && [ -w "$BIN_DIR" ]; then
    install -m 0755 "$TARGET" "$DEST"
else
    sudo install -d "$BIN_DIR"
    sudo install -m 0755 "$TARGET" "$DEST"
fi

if [ -f "$PROFILE_FILE" ]; then
    TMP_PROFILE="${PROFILE_FILE}.mem012.tmp.$$"
    grep -Fv 'export MEM012_CONFIG=' "$PROFILE_FILE" > "$TMP_PROFILE" || true
    printf '\n%s' "$EXPORT_LINE" >> "$TMP_PROFILE"
    mv "$TMP_PROFILE" "$PROFILE_FILE"
else
    printf '%s\n' "$EXPORT_LINE" > "$PROFILE_FILE"
fi

printf 'installed: %s\n' "$DEST"
printf 'configured: %s\n' "$PROFILE_FILE"
