#!/bin/bash
# 把当前这套配置封成一个可分发的包：安装包去掉大资产，加上取资产与安装的脚本、清单与说明。
#
# 用法
#   code/scripts/package-release.sh                 # 产出到 dist/
#   code/scripts/package-release.sh --out DIR       # 产出到指定目录
#   code/scripts/package-release.sh --with-assets   # 连同权重与 metallib 一起封（本机自用，很大）
#
# 产出
#   dist/RimeClient-<日期>.tar.zst   不含权重与 metallib 的 app，加脚本与说明
#   dist/RimeClient-<日期>.sha256    上述归档的 sha256
#
# 为什么不把权重封进去：weights.safetensors 是 513 MB，超过 GitHub 单文件上限；
# 归档里带的是 fetch-model.sh，装机时按清单里的哈希下载。
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
SOURCE_APP="${SOURCE_APP:-$ROOT/code/Client/dist/Squirrel.app}"
OUT_DIR="$ROOT/dist"
WITH_ASSETS=0

while [ $# -gt 0 ]; do
  case "$1" in
    --out) shift; OUT_DIR="$1" ;;
    --with-assets) WITH_ASSETS=1 ;;
    *) printf 'package-release.sh: 未知参数 %s\n' "$1" >&2; exit 2 ;;
  esac
  shift
done

fail() { printf 'package-release.sh: %s\n' "$*" >&2; exit 1; }
say() { printf '%s\n' "$*"; }

[ -d "$SOURCE_APP" ] || fail "找不到 $SOURCE_APP，先跑 code/Client/scripts/make-app.sh"
[ -x "$SOURCE_APP/Contents/MacOS/Squirrel" ] || fail "安装包里没有可执行文件"

VERSION="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$SOURCE_APP/Contents/Info.plist" 2>/dev/null || echo 0)"
STAMP="$(date +%Y%m%d)"
NAME="RimeClient-$STAMP"
STAGE="$OUT_DIR/$NAME"
mkdir -p "$OUT_DIR"
rm -rf "$STAGE"
mkdir -p "$STAGE"

say "== 复制安装包"
ditto "$SOURCE_APP" "$STAGE/RimeClient.app"

if [ "$WITH_ASSETS" -eq 1 ]; then
  say "== 保留权重与 metallib（--with-assets）"
else
  say "== 去掉权重与 metallib：由 fetch-model.sh 在装机时取"
  rm -rf "$STAGE/RimeClient.app/Contents/Resources/Models"
  rm -f "$STAGE/RimeClient.app/Contents/Resources/mlx.metallib"
  # 去掉大资产后原签名失效，重签一次，保证解开就能跑。
  codesign --force --deep --sign - \
    --entitlements "$STAGE/RimeClient.app/Contents/Resources/entitlements.plist" \
    "$STAGE/RimeClient.app" >/dev/null 2>&1 || fail "重签失败"
fi

say "== 放入脚本"
mkdir -p "$STAGE/scripts"
cp "$ROOT/code/scripts/fetch-model.sh" "$STAGE/scripts/"
cp "$ROOT/code/scripts/install-app.sh" "$STAGE/scripts/"
cp "$ROOT/code/Client/scripts/make-app.sh" "$STAGE/scripts/" 2>/dev/null || true
cp "$ROOT/code/Client/BUILD.txt" "$STAGE/scripts/" 2>/dev/null || true
chmod +x "$STAGE/scripts/"*.sh

say "== 写入说明"
cat > "$STAGE/INSTALL.md" <<EOF
# RimeClient ${VERSION}（${STAMP} 封装）

这套包是当前工作机的配置快照。键盘走编译进包的 librime，语音走进程内的 Rust 引擎与 MLX（Qwen3-ASR 0.6B 4-bit）。

## 里面有什么

- \`RimeClient.app\`：输入法包。**不含**模型权重与 \`mlx.metallib\`（两者合计约 690 MB，超过 git 单文件上限）。
- \`scripts/fetch-model.sh\`：按清单里的 sha256 取权重与 metallib。
- \`scripts/install-app.sh\`：停进程 → 复制到 \`~/Library/Input Methods\` → 校验签名 → 核对包内资产。
- \`MANIFEST.sha256\`：本包内所有文件的哈希。

## 装到一台 Apple 芯片的 Mac

\`\`\`sh
tar --zstd -xf RimeClient-${STAMP}.tar.zst
cd ${NAME}
scripts/fetch-model.sh --dest "$PWD/assets"
scripts/install-app.sh --assets "$PWD/assets"
\`\`\`

装完在输入法菜单里选 **RimeClient**。键盘中文需要 \`~/Library/Rime\` 里已有方案；这个包不附带方案。

## 前提

- Apple 芯片（arm64）与 macOS 14 以上。Intel 上没有可用的数值内核。
- 首次按键前引擎会读一次权重；已做预热，进程起来后约 0.14 秒可接受按键。
- 语音键是按住右 Option，松开提交定稿。

## 不保证

- 本包是本机签名（adhoc，无 Team ID）。另一台机器若拒绝打开，需要在系统设置里允许。
- 模型权重与 metallib 不在包里，必须能访问 Hugging Face。
EOF

say "== 记录资产哈希（若本机有）"
if [ -f "$ROOT/code/assets/qwen3-asr-0.6b-4bit/weights.safetensors" ] && [ "$WITH_ASSETS" -eq 0 ]; then
  shasum -a 256 "$ROOT/code/assets/qwen3-asr-0.6b-4bit/weights.safetensors" \
    | awk '{print $1"  qwen3-asr-0.6b-4bit/weights.safetensors"}' >> "$STAGE/MANIFEST.assets.sha256" || true
fi

say "== 写清单"
( cd "$STAGE" && find . -type f -not -name 'MANIFEST.sha256' -print0 \
    | sort -z | xargs -0 shasum -a 256 > MANIFEST.sha256 )

say "== 打包"
ARCHIVE="$OUT_DIR/$NAME.tar.zst"
rm -f "$ARCHIVE"
COPYFILE_DISABLE=1 tar --zstd -cf "$ARCHIVE" -C "$OUT_DIR" "$NAME"
shasum -a 256 "$ARCHIVE" | awk '{print $1}' > "$OUT_DIR/$NAME.sha256"

say ""
say "归档：${ARCHIVE}（$(du -h "$ARCHIVE" | awk '{print $1}')）"
say "哈希：$(cat "${OUT_DIR}/${NAME}.sha256")"
