#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

validate_target_dir() {
  # What：要求内部部署脚本接收且仅接收一个安全的绝对目标目录。
  # Why：清理操作必须使用规范化路径，不能让包含 .. 的输入绕过根目录保护。
  if (( $# != 1 )); then
    echo "[ERROR] 用法: deploy.sh /绝对目标目录" >&2
    return 2
  fi
  case $1 in
    ""|[!/]*) echo "[ERROR] 目标目录必须是非空绝对路径，且不能是 /: $1" >&2; return 2 ;;
  esac
  local normalized
  normalized=$(
    TARGET_INPUT=$1 node <<'NODE'
const fs = require("fs");
const path = require("path");
let current = path.resolve(process.env.TARGET_INPUT);
const tail = [];
while (!fs.existsSync(current)) {
  tail.unshift(path.basename(current));
  const parent = path.dirname(current);
  if (parent === current) break;
  current = parent;
}
process.stdout.write(path.join(fs.realpathSync(current), ...tail));
NODE
  )
  if [[ "$normalized" == "/" ]]; then
    echo "[ERROR] 目标目录规范化后不能是 /: $1" >&2
    return 2
  fi
  printf '%s' "$normalized"
}

publish_dist() {
  # What：清空目标目录并复制构建产物，仅在当前用户无写权限时提权。
  # Why：npm 不应以 root 运行，但默认 /opt 发布目录通常必须使用管理员权限。
  local privileged=false
  if ! mkdir -p -- "$TARGET_DIR" 2>/dev/null || [[ ! -w "$TARGET_DIR" ]]; then
    sudo install -d "$TARGET_DIR"
    privileged=true
  fi

  echo "[3/4] 清空目标目录: $TARGET_DIR"
  if $privileged; then
    sudo find "$TARGET_DIR" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +
  else
    find "$TARGET_DIR" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +
  fi

  echo "[4/4] 复制 dist 到目标目录"
  if $privileged; then
    sudo cp -a "$SCRIPT_DIR/dist/." "$TARGET_DIR/"
  else
    cp -a "$SCRIPT_DIR/dist/." "$TARGET_DIR/"
  fi
}

TARGET_DIR=$(validate_target_dir "$@")

cd "$SCRIPT_DIR"
NPM_CACHE_DIR="${NPM_CONFIG_CACHE:-${TMPDIR:-/tmp}/mem012-frontend-npm-cache}"

echo "[1/4] 安装前端依赖..."
npm ci --cache "$NPM_CACHE_DIR"

echo "[2/4] 构建前端..."
npm run build

if [[ ! -f "$SCRIPT_DIR/dist/index.html" ]]; then
  echo "[ERROR] 未找到 dist/index.html，构建失败" >&2
  exit 1
fi

publish_dist

echo "[DONE] 发布完成 -> $TARGET_DIR"
