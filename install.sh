#!/usr/bin/env sh
set -eu

ROOT_DIR="$(pwd)"
BIN_DIR=${BIN_DIR:-/usr/local/bin}
TARGET="$ROOT_DIR/target/release/mem_012"
DEST="$BIN_DIR/mem012"
CONFIG_PATH=${MEM012_CONFIG:-"$ROOT_DIR/config.toml"}
PROFILE_FILE="$HOME/.bashrc"
EXPORT_LINE="export MEM012_CONFIG=\"$CONFIG_PATH\""

write_profile_config_line() {
    profile_file=$1
    export_line=$2

    # What：移除旧 MEM012_CONFIG 行后，把新配置行稳定写到 shell rc 文件末尾。
    # Why：重复安装必须幂等，不能每次在用户的 .zshrc/.bashrc 末尾多留一个空行。
    if [ -f "$profile_file" ]; then
        TMP_PROFILE="${profile_file}.mem012.tmp.$$"
        awk '
            /^[[:space:]]*export[[:space:]]+MEM012_CONFIG=/ { next }
            { lines[++count] = $0 }
            END {
                while (count > 0 && lines[count] == "") count--
                for (i = 1; i <= count; i++) print lines[i]
            }
        ' "$profile_file" > "$TMP_PROFILE"
        if [ -s "$TMP_PROFILE" ]; then
            printf '\n%s\n' "$export_line" >> "$TMP_PROFILE"
        else
            printf '%s\n' "$export_line" >> "$TMP_PROFILE"
        fi
        mv "$TMP_PROFILE" "$profile_file"
    else
        printf '%s\n' "$export_line" > "$profile_file"
    fi
}

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

write_profile_config_line "$PROFILE_FILE" "$EXPORT_LINE"

printf 'installed: %s\n' "$DEST"
printf 'configured: %s\n' "$PROFILE_FILE"
