#!/usr/bin/env sh
set -eu

ROOT_DIR="$(pwd)"
BIN_DIR=${BIN_DIR:-/usr/local/bin}
TARGET="$ROOT_DIR/target/release/mem_012"
DEST="$BIN_DIR/mem012"
CONFIG_PATH=${MEM012_CONFIG:-"$ROOT_DIR/config.toml"}
PROFILE_FILE="$HOME/.bashrc"
EXPORT_LINE="export MEM012_CONFIG=\"$CONFIG_PATH\""
DEFAULT_FRONTEND_DIR=/opt/1panel/www/sites/mem012/index
INSTALL_MODE=all
FRONTEND_DIR=$DEFAULT_FRONTEND_DIR

parse_install_args() {
    # What：把安装参数归一化为 all、frontend 或 backend，并校验前端目标目录。
    # Why：在执行任何构建或系统操作前拒绝歧义输入，避免错误模式产生部分安装。
    case $# in
        0) ;;
        1)
            case $1 in
                --frontend) INSTALL_MODE=frontend ;;
                --backend) INSTALL_MODE=backend ;;
                *) printf '未知参数：%s\n' "$1" >&2; return 2 ;;
            esac
            ;;
        2)
            if [ "$1" != --frontend ]; then
                printf '%s\n' '--backend 不接受附加参数，且 --frontend 与 --backend 互斥。' >&2
                return 2
            fi
            case $2 in
                ''|/|[!/]*) printf '前端安装目录必须是非空绝对路径，且不能是 /：%s\n' "$2" >&2; return 2 ;;
                *) INSTALL_MODE=frontend; FRONTEND_DIR=$2 ;;
            esac
            ;;
        *) printf '%s\n' '参数过多。用法：install.sh [--frontend [绝对路径] | --backend]' >&2; return 2 ;;
    esac
}

parse_install_args "$@"

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

install_server_service() {
    service_file="${TMPDIR:-/tmp}/mem012.service.$$"

    # What：安装并启用由当前用户运行的 mem012 systemd 服务。
    # Why：服务必须复用安装时确定的二进制和配置绝对路径，不能依赖登录 shell 环境。
    [ "$(uname -s)" = "Linux" ] || return 0
    if ! command -v systemctl >/dev/null 2>&1; then
        printf '%s\n' '安装失败：Linux 系统未提供 systemctl，无法安装 mem012.service。' >&2
        return 1
    fi
    {
        printf '%s\n' '[Unit]' 'Description=mem012 server' 'Wants=network-online.target' 'After=network-online.target' ''
        printf '%s\n' '[Service]' 'Type=simple'
        printf 'User=%s\nGroup=%s\n' "$(id -un)" "$(id -gn)"
        printf 'Environment="MEM012_CONFIG=%s"\n' "$CONFIG_PATH"
        printf 'ExecStart="%s" server\n' "$DEST"
        printf '%s\n' 'Restart=on-failure' 'RestartSec=5' '' '[Install]' 'WantedBy=multi-user.target'
    } > "$service_file"
    if ! sudo install -m 0644 "$service_file" /etc/systemd/system/mem012.service; then
        rm -f "$service_file"
        printf '%s\n' '安装失败：无法写入 /etc/systemd/system/mem012.service。' >&2
        return 1
    fi
    rm -f "$service_file"
    if sudo systemctl daemon-reload \
        && sudo systemctl enable --now mem012.service \
        && sudo systemctl is-active --quiet mem012.service; then
        printf '%s\n' '持久化服务已启动：mem012.service'
    else
        printf '%s\n' '安装失败：mem012.service 持久化启动失败，请检查 systemctl 日志。' >&2
        return 1
    fi
}

install_backend() {
    # What：构建并安装后端二进制、shell 配置和 systemd 服务。
    # Why：后端副作用需要一个明确边界，后续才能按安装模式选择性执行。
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
    install_server_service
    printf 'installed: %s\n' "$DEST"
    printf 'configured: %s\n' "$PROFILE_FILE"
    printf '[DONE] 后端安装完成 -> %s\n' "$DEST"
}

install_frontend() {
    # What：调用内部部署脚本，把前端发布到已校验的目标目录。
    # Why：顶层脚本是唯一用户入口，内部脚本不应自行推断生产路径。
    bash "$ROOT_DIR/frontend/deploy.sh" "$FRONTEND_DIR"
}

run_install() {
    # What：按已解析的安装模式选择前端、后端或完整安装流程。
    # Why：模式分流必须集中在单一入口，确保各模式不会产生越界副作用。
    case $INSTALL_MODE in
        all) install_backend; install_frontend ;;
        frontend) install_frontend ;;
        backend) install_backend ;;
    esac
}

run_install
