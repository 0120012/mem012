#!/usr/bin/env bash
set -euo pipefail

# Why: 固定发布入口，避免手工构建和复制时把 dist 发到错误站点目录。
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEFAULT_TARGET="/opt/1panel/www/sites/devm/index"
TARGET_DIR="${1:-$DEFAULT_TARGET}"

if [[ ! -d "$TARGET_DIR" ]]; then
  echo "[ERROR] 目标目录不存在: $TARGET_DIR" >&2
  exit 1
fi

if [[ "$TARGET_DIR" == "/" ]]; then
  echo "[ERROR] 拒绝发布到根目录 /" >&2
  exit 1
fi

cd "$SCRIPT_DIR"

echo "[1/3] 构建前端..."
npm run build

if [[ ! -d "$SCRIPT_DIR/dist" ]]; then
  echo "[ERROR] 未找到 dist 目录，构建失败" >&2
  exit 1
fi

echo "[2/3] 清空目标目录: $TARGET_DIR"
shopt -s dotglob nullglob
old_files=("$TARGET_DIR"/*)
if (( ${#old_files[@]} > 0 )); then
  rm -rf -- "${old_files[@]}"
fi

echo "[3/3] 复制 dist 到目标目录"
cp -a "$SCRIPT_DIR/dist/." "$TARGET_DIR/"

echo "[DONE] 发布完成 -> $TARGET_DIR"
