#!/bin/bash
# 把 dist 里的包装进 ~/Library/Input Methods/RimeClient.app。
# 顺序固定：停进程 → ditto → codesign --verify --deep --strict → 核对包内资产。
# 任何一步不过就退出，不留下装了一半的包。
#
# 用法
#   code/scripts/install-app.sh                      # 装 code/Client/dist/Squirrel.app
#   code/scripts/install-app.sh /tmp/X.app           # 装指定包
#   code/scripts/install-app.sh --dest DIR           # 装到 DIR/RimeClient.app
#   code/scripts/install-app.sh --no-stop            # 不动正在跑的进程（调试用）
#   code/scripts/install-app.sh --assets DIR         # 装之前把 DIR 里的权重与 metallib 补进包（封装包用这个）
#   code/scripts/install-app.sh --rime-config DIR    # 把 DIR/Rime 的配置播种到 ~/Library/Rime（仅当它为空）
#   code/scripts/install-app.sh --rime-config DIR --force-rime  # 覆盖已有配置（先备份）
#   code/scripts/install-app.sh --rebuild-rime       # 删掉 ~/Library/Rime/build 再部署（换过词库或 librime 版本后必做）
#
# 环境变量：INSTALL_DEST（默认 "$HOME/Library/Input Methods"）、SOURCE_APP。
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
# 默认包位置：先看脚本旁边的 RimeClient.app（封装包里的布局），再看仓库里的构建产物。
if [ -n "${SOURCE_APP:-}" ]; then
  SRC="$SOURCE_APP"
elif [ -d "$HERE/../RimeClient.app" ]; then
  SRC="$HERE/../RimeClient.app"
else
  SRC="$ROOT/code/Client/dist/Squirrel.app"
fi
DEST_DIR="${INSTALL_DEST:-$HOME/Library/Input Methods}"
NAME="RimeClient.app"
STOP=1
ASSETS=""
RIME_CONFIG=""
FORCE_RIME=0
REBUILD_RIME=0

fail() {
  printf 'install-app.sh: %s\n' "$*" >&2
  exit 1
}

say() {
  printf '%s\n' "$*"
}

usage() {
  sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'
  exit 0
}

while [ $# -gt 0 ]; do
  case "$1" in
    --dest)
      [ $# -ge 2 ] || fail "--dest 后面要给目录"
      DEST_DIR="$2"
      shift 2
      ;;
    --no-stop)
      STOP=0
      shift
      ;;
    --assets)
      [ $# -ge 2 ] || fail "--assets 后面要给目录"
      ASSETS="$2"
      shift 2
      ;;
    --rime-config)
      [ $# -ge 2 ] || fail "--rime-config 后面要给目录"
      RIME_CONFIG="$2"
      shift 2
      ;;
    --force-rime)
      FORCE_RIME=1
      shift
      ;;
    --rebuild-rime)
      REBUILD_RIME=1
      shift
      ;;
    -h | --help)
      usage
      ;;
    -*)
      fail "不认识的参数：$1（--help 看用法）"
      ;;
    *)
      SRC="$1"
      shift
      ;;
  esac
done

[ -d "$SRC" ] || fail "没有包：$SRC
先打包：code/Client/scripts/make-app.sh"
[ -f "$SRC/Contents/Info.plist" ] || fail "$SRC 里没有 Contents/Info.plist；这不是应用包"
DEST="$DEST_DIR/$NAME"

# 1. 停进程。输入法进程被 launchd 拉起，杀掉之后系统会在下次切过去时再起。
if [ "$STOP" -eq 1 ] && [ -d "$DEST" ]; then
  if pgrep -f "$DEST/Contents/MacOS/Squirrel" >/dev/null 2>&1; then
    say "停进程：$DEST/Contents/MacOS/Squirrel"
    pkill -f "$DEST/Contents/MacOS/Squirrel" || true
    for _ in $(seq 1 50); do
      pgrep -f "$DEST/Contents/MacOS/Squirrel" >/dev/null 2>&1 || break
      sleep 0.1
    done
    pgrep -f "$DEST/Contents/MacOS/Squirrel" >/dev/null 2>&1 &&
      fail "进程没停下来：$DEST/Contents/MacOS/Squirrel（手动退出输入法后重试）"
  fi
fi

# 2. ditto 覆盖。先删旧的，避免上一版留下的文件混在里面。
mkdir -p "$DEST_DIR"
rm -rf "$DEST"
say "拷贝：$SRC → $DEST"
ditto "$SRC" "$DEST"
[ -d "$DEST" ] || fail "ditto 没产出 $DEST"

# 3. 签名必须过严格校验。
if [ -n "$ASSETS" ]; then
  say "补资产：$ASSETS"
  [ -f "$ASSETS/mlx.metallib" ] || fail "$ASSETS 里没有 mlx.metallib（先跑 fetch-model.sh）"
  [ -f "$ASSETS/qwen3-asr-0.6b-4bit/weights.safetensors" ] || fail "$ASSETS 里没有 qwen3-asr-0.6b-4bit/weights.safetensors"
  mkdir -p "$DEST/Contents/Resources/Models/Qwen3-ASR-0.6B-4bit"
  cp -L "$ASSETS/mlx.metallib" "$DEST/Contents/Resources/mlx.metallib"
  for file in config.json merges.txt quantization_config.json tokenizer_config.json vocab.json weights.safetensors; do
    [ -f "$ASSETS/qwen3-asr-0.6b-4bit/$file" ] || fail "$ASSETS 缺 $file"
    cp -L "$ASSETS/qwen3-asr-0.6b-4bit/$file" "$DEST/Contents/Resources/Models/Qwen3-ASR-0.6B-4bit/$file"
  done
  say "补完资产后重签"
  codesign --force --deep --sign - \
    --entitlements "$DEST/Contents/Resources/entitlements.plist" "$DEST" >/dev/null 2>&1 ||
    fail "补资产后重签失败"
fi

say "校验签名：codesign --verify --deep --strict"
codesign --verify --deep --strict "$DEST" ||
  fail "codesign --verify --deep --strict 没过：$DEST（重新打包：code/Client/scripts/make-app.sh）"

# 4. 包内资产必须在。
for f in Contents/Info.plist \
  Contents/Resources/mlx.metallib \
  Contents/Resources/Models/Qwen3-ASR-0.6B-4bit/weights.safetensors \
  Contents/Resources/Models/Qwen3-ASR-0.6B-4bit/vocab.json; do
  [ -f "$DEST/$f" ] || fail "装好的包里缺 $f"
  [ -s "$DEST/$f" ] || fail "装好的包里 $f 是空文件"
  say "在  $f  ($(stat -f%z "$DEST/$f") 字节)"
done

# 打包装时把 fetch-model.sh 的清单拷了进来，这里顺手复核。
if [ -f "$DEST/Contents/Resources/voice-assets.sha256" ]; then
  ( cd "$DEST/Contents/Resources" &&
    awk '{print $1"  "$2}' voice-assets.sha256 |
      sed 's#  qwen3-asr-0.6b-4bit/#  Models/Qwen3-ASR-0.6B-4bit/#' |
      shasum -a 256 -c --quiet - ) ||
    fail "包内资产与 voice-assets.sha256 不一致；重新打包"
  say "资产哈希与包内清单一致"
fi

say ""
if [ "$REBUILD_RIME" -eq 1 ]; then
  USER_RIME="${RIME_USER_DIR:-$HOME/Library/Rime}"
  if [ -d "$USER_RIME/build" ]; then
    say "删掉编译缓存：$USER_RIME/build（只是产物，部署会重建）"
    rm -rf "$USER_RIME/build"
  fi
  say "重新部署（首次编译词库可能要几分钟，那个几百 MB 的语言模型最慢）"
  RIME_USER_DIR="$USER_RIME" "$DEST/Contents/MacOS/Squirrel" --build | tail -1
fi

if [ -n "$RIME_CONFIG" ]; then
  [ -d "$RIME_CONFIG/Rime" ] || fail "$RIME_CONFIG/Rime 不存在（先跑 export-rime-config.sh）"
  USER_RIME="$HOME/Library/Rime"
  if [ -d "$USER_RIME" ] && [ -n "$(ls -A "$USER_RIME" 2>/dev/null)" ]; then
    if [ "$FORCE_RIME" -eq 1 ]; then
      BACKUP="$USER_RIME.backup.$(date +%Y%m%d%H%M%S)"
      say "备份已有配置：$BACKUP"
      mv "$USER_RIME" "$BACKUP"
      mkdir -p "$USER_RIME"
    else
      say "~/Library/Rime 已有内容，跳过播种（要覆盖加 --force-rime，会先备份）"
    fi
  else
    mkdir -p "$USER_RIME"
  fi
  if [ -z "$(ls -A "$USER_RIME" 2>/dev/null)" ] || [ "$FORCE_RIME" -eq 1 ]; then
    say "播种配置：$RIME_CONFIG/Rime -> $USER_RIME"
    cp -R "$RIME_CONFIG/Rime/." "$USER_RIME/"
  fi
  say "部署（首次会编译词库，可能要几分钟）"
  "$DEST/Contents/MacOS/Squirrel" --build >/dev/null 2>&1 || say "部署没有成功返回，可在输入法菜单里点「重新部署」再试"
fi

say "装好了：$DEST"
say "启用输入源："
say "  \"$DEST/Contents/MacOS/Squirrel\" --register-input-source"
say "  \"$DEST/Contents/MacOS/Squirrel\" --enable-input-source"