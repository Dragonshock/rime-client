#!/bin/bash
# 把本机 ~/Library/Rime 的「源配置」导出成一个可分发目录：解开符号链接，丢掉编译产物与个人数据。
#
# 用法
#   code/scripts/export-rime-config.sh                 # 导出到 dist/rime-config
#   code/scripts/export-rime-config.sh --out DIR
#   code/scripts/export-rime-config.sh --with-personal # 连 custom_phrase.txt 一起导出（默认排除）
#
# 排除项与理由
#   build/           编译产物，由部署重建
#   *.userdb/        个人词频与用户词，属隐私
#   installation.yaml 安装标识，换机器应当重生成
#   custom_phrase.txt 常有私人地址与常用语，默认不导出
#   others/          语料原文（约 95 MB），部署时用不到；词库已含在 .dict.yaml 里
#
# 产物
#   <out>/Rime/...    解开符号链接后的源文件
#   <out>/MANIFEST.sha256
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
SRC="${RIME_USER_DIR:-$HOME/Library/Rime}"
OUT_DEFAULT="$ROOT/dist/rime-config"
OUT="$OUT_DEFAULT"
WITH_PERSONAL=0

while [ $# -gt 0 ]; do
  case "$1" in
    --out) shift; OUT="$1" ;;
    --with-personal) WITH_PERSONAL=1 ;;
    -h | --help) sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) printf 'export-rime-config.sh: 未知参数 %s\n' "$1" >&2; exit 2 ;;
  esac
  shift
done

fail() { printf 'export-rime-config.sh: %s\n' "$*" >&2; exit 1; }
say() { printf '%s\n' "$*"; }

[ -d "$SRC" ] || fail "找不到 $SRC"

rm -rf "$OUT"
mkdir -p "$OUT/Rime"

say "导出源配置：$SRC -> $OUT/Rime"
# -L 解开符号链接；-R 递归；排除编译产物与个人数据。
rsync -rL --quiet \
  --exclude 'build/' \
  --exclude '*.userdb/' \
  --exclude 'installation.yaml' \
  --exclude 'others/' \
  --exclude '.git/' \
  --exclude '.github/' \
  "$SRC/" "$OUT/Rime/"

if [ "$WITH_PERSONAL" -eq 0 ]; then
  rm -f "$OUT/Rime/custom_phrase.txt"
  say "已排除 custom_phrase.txt（要带上它加 --with-personal）"
fi

# 分发时不需要的东西：本地文档目录、锁文件。
rm -rf "$OUT/Rime/dev-docs" "$OUT/Rime/.gitignore" 2>/dev/null || true

count=$(find "$OUT/Rime" -type f | wc -l | tr -d ' ')
bytes=$(du -sh "$OUT/Rime" | awk '{print $1}')
say "文件数 ${count}，大小 ${bytes}"

( cd "$OUT" && find Rime -type f -print0 | sort -z | xargs -0 shasum -a 256 > MANIFEST.sha256 )
say "清单：$OUT/MANIFEST.sha256"

tar --zstd -cf "$OUT/rime-config.tar.zst" -C "$OUT" Rime MANIFEST.sha256
say "归档：$OUT/rime-config.tar.zst（$(du -h "$OUT/rime-config.tar.zst" | awk '{print $1}')）"
