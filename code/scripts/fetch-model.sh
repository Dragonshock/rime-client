#!/bin/bash
# 把 0.6B 4-bit 权重与 mlx.metallib 取到一个目录里，并逐个文件核对 sha256。
# 大文件不进 git：这个脚本只下载到本地目录（默认 code/assets，已进 code/.gitignore）。
#
# 用法
#   code/scripts/fetch-model.sh                      # 取到 code/assets
#   code/scripts/fetch-model.sh --dest /tmp/voice    # 取到指定目录
#   code/scripts/fetch-model.sh --verify-only        # 不联网，只核对目录里现有文件
#   code/scripts/fetch-model.sh --metallib ~/mlx.metallib
#   code/scripts/fetch-model.sh --metallib-from-build
#   code/scripts/fetch-model.sh --accept-new-metallib
#   code/scripts/fetch-model.sh --force              # 哈希不对时删掉重下
#   code/scripts/fetch-model.sh --weights-only       # 只取六个权重文件，不碰 metallib
#
# 目录形状（make-app.sh 只认这两条路径，不再用 find 猜）
#   <dest>/qwen3-asr-0.6b-4bit/{config.json,merges.txt,quantization_config.json,
#                              tokenizer_config.json,vocab.json,weights.safetensors}
#   <dest>/mlx.metallib
#   <dest>/MANIFEST.sha256     # 本次落盘的实际哈希，装包时用得上
#
# 权重来源：Hugging Face moona3k/mlx-qwen3-asr-0.6b-4bit，
# 修订 4c59c533f95c84afb796655e814709034f826f04（不可变的 commit，不用 main）。
#
# 校验值（2026-11-14 取自本机 HF 快照与已安装包的 mlx.metallib）：
#   config.json                6193        76d3ae4601ce939830b2517f4a6cadb86cc51316c3900af6b020b051c21a478c
#   merges.txt                 1671853     8831e4f1a044471340f7c0a83d7bd71306a5b867e95fd870f74d0c5308a904d5
#   quantization_config.json   60          067339b858243ad9d5e22764b3b82c759a765c9f0aadfe4939d0c233f15b9e7d
#   tokenizer_config.json      12487       4942d005604266809309cabc9f4e9cb89ce855d59b14681fdc0e1cc62ea26c4c
#   vocab.json                 2776833     ca10d7e9fb3ed18575dd1e277a2579c16d108e32f27439684afa0e10b1440910
#   weights.safetensors        537626967   9a62bb9830dcbcd0cf1c1f40200161d4e5f007a4cd99b83111902890a9f087d3
#   mlx.metallib               183870464   d523d6022efced2817e305fdca37c325868f670d96487aaf24a6864bcda3c722
#
# mlx.metallib 不在 Hugging Face 上：2026-11-14 用 HF tree API 核对该仓库只有 8 个文件
# （上面 6 个加 .gitattributes 与 README.md），没有 metallib 镜像。它是 MLX 的 C++ 内核
# 编译产物，由 `cargo build -p voice-ffi --features metal` 里的 mlx-sys 生成，因此这份
# 文件只有两个合法来源：显式给出的路径（--metallib / ${VOICE_METALLIB}）或本机构建目录
# （--metallib-from-build）。换一台机器重新编译出来的 metallib 哈希会变，所以脚本默认
# 拒绝与下面这个值不同的文件；确认那是新编译的产物，再加 --accept-new-metallib。
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
REPO="moona3k/mlx-qwen3-asr-0.6b-4bit"
REVISION="4c59c533f95c84afb796655e814709034f826f04"
BASE_URL="https://huggingface.co/$REPO/resolve/$REVISION"
MODEL_SUBDIR="qwen3-asr-0.6b-4bit"
METALLIB_SHA256="d523d6022efced2817e305fdca37c325868f670d96487aaf24a6864bcda3c722"
METALLIB_BYTES=183870464

# name | sha256 | bytes
MODEL_FILES=(
  "config.json 76d3ae4601ce939830b2517f4a6cadb86cc51316c3900af6b020b051c21a478c 6193"
  "merges.txt 8831e4f1a044471340f7c0a83d7bd71306a5b867e95fd870f74d0c5308a904d5 1671853"
  "quantization_config.json 067339b858243ad9d5e22764b3b82c759a765c9f0aadfe4939d0c233f15b9e7d 60"
  "tokenizer_config.json 4942d005604266809309cabc9f4e9cb89ce855d59b14681fdc0e1cc62ea26c4c 12487"
  "vocab.json ca10d7e9fb3ed18575dd1e277a2579c16d108e32f27439684afa0e10b1440910 2776833"
  "weights.safetensors 9a62bb9830dcbcd0cf1c1f40200161d4e5f007a4cd99b83111902890a9f087d3 537626967"
)

DEST="${VOICE_ASSETS:-$ROOT/code/assets}"
METALLIB_SRC="${VOICE_METALLIB:-}"
FROM_BUILD=0
ACCEPT_NEW_METALLIB=0
VERIFY_ONLY=0
FORCE=0
WEIGHTS_ONLY=0

die() {
  printf 'fetch-model.sh: %s\n' "$*" >&2
  exit 1
}

say() {
  printf '%s\n' "$*"
}

usage() {
  sed -n '2,38p' "$0" | sed 's/^# \{0,1\}//'
  exit 0
}

while [ $# -gt 0 ]; do
  case "$1" in
    --dest)
      [ $# -ge 2 ] || die "--dest 后面要给目录"
      DEST="$2"
      shift 2
      ;;
    --dest=*)
      DEST="${1#--dest=}"
      shift
      ;;
    --metallib)
      [ $# -ge 2 ] || die "--metallib 后面要给文件路径"
      METALLIB_SRC="$2"
      shift 2
      ;;
    --metallib=*)
      METALLIB_SRC="${1#--metallib=}"
      shift
      ;;
    --metallib-from-build)
      FROM_BUILD=1
      shift
      ;;
    --accept-new-metallib)
      ACCEPT_NEW_METALLIB=1
      shift
      ;;
    --verify-only)
      VERIFY_ONLY=1
      shift
      ;;
    --force)
      FORCE=1
      shift
      ;;
    --weights-only)
      WEIGHTS_ONLY=1
      shift
      ;;
    -h | --help)
      usage
      ;;
    *)
      die "不认识的参数：$1（--help 看用法）"
      ;;
  esac
done

hash_of() {
  shasum -a 256 "$1" | cut -d' ' -f1
}

bytes_of() {
  stat -f%z "$1"
}

check_one() {
  # $1 文件路径，$2 期望哈希，$3 期望字节数，$4 显示名
  local path="$1" want="$2" want_bytes="$3" label="$4"
  if [ ! -f "$path" ]; then
    die "缺文件：${path}（${label}）"
  fi
  if [ -L "$path" ]; then
    die "$path 是符号链接（${label}）；请给出真实文件，不要把 HF 快照里的链接拷过来"
  fi
  local got got_bytes
  got="$(hash_of "$path")"
  got_bytes="$(bytes_of "$path")"
  if [ "$got" != "$want" ]; then
    die "$label 的 sha256 不对：实际 ${got}（$got_bytes 字节），脚本记录 ${want}（$want_bytes 字节）；文件 $path"
  fi
  if [ "$got_bytes" != "$want_bytes" ]; then
    die "$label 的字节数不对：实际 ${got_bytes}，脚本记录 ${want_bytes}；文件 $path"
  fi
}

record_manifest() {
  # 落盘实际哈希，装包时可用 shasum -c 复核
  local manifest="$DEST/MANIFEST.sha256"
  : >"$manifest"
  local i line name
  for i in "${MODEL_FILES[@]}"; do
    # shellcheck disable=SC2086
    set -- $i
    name="$1"
    printf '%s  %s\n' "$(hash_of "$DEST/$MODEL_SUBDIR/$name")" "$MODEL_SUBDIR/$name" >>"$manifest"
  done
  if [ -f "$DEST/mlx.metallib" ]; then
    printf '%s  %s\n' "$(hash_of "$DEST/mlx.metallib")" "mlx.metallib" >>"$manifest"
  fi
  say "清单：$manifest"
}

mkdir -p "$DEST" "$DEST/$MODEL_SUBDIR"

if [ "$VERIFY_ONLY" -eq 1 ]; then
  for i in "${MODEL_FILES[@]}"; do
    # shellcheck disable=SC2086
    set -- $i
    check_one "$DEST/$MODEL_SUBDIR/$1" "$2" "$3" "$1"
    say "ok  $MODEL_SUBDIR/$1"
  done
  if [ -f "$DEST/mlx.metallib" ]; then
    got="$(hash_of "$DEST/mlx.metallib")"
    if [ "$got" = "$METALLIB_SHA256" ]; then
      say "ok  mlx.metallib"
    else
      say "注意  mlx.metallib 与脚本记录不同：$got（记录 $METALLIB_SHA256）；如果是另一次编译的产物，用 --metallib-from-build --accept-new-metallib 重新落一次清单"
    fi
  else
    say "缺  mlx.metallib（权重六个文件都在）；补：--metallib-from-build"
  fi
  if [ -f "$DEST/MANIFEST.sha256" ]; then
    ( cd "$DEST" && shasum -a 256 -c --quiet MANIFEST.sha256 ) ||
      die "清单核对失败：$DEST/MANIFEST.sha256"
    say "清单核对通过：$DEST/MANIFEST.sha256"
  fi
  exit 0
fi

command -v curl >/dev/null || die "找不到 curl"
command -v shasum >/dev/null || die "找不到 shasum"

for i in "${MODEL_FILES[@]}"; do
  # shellcheck disable=SC2086
  set -- $i
  name="$1"
  want_sha="$2"
  want_bytes="$3"
  target="$DEST/$MODEL_SUBDIR/$name"

  if [ -f "$target" ] && [ ! -L "$target" ] && [ "$(hash_of "$target")" = "$want_sha" ]; then
    say "已有  $MODEL_SUBDIR/$name"
    continue
  fi
  if [ -f "$target" ] && [ "$FORCE" -eq 0 ]; then
    die "$target 已存在但哈希不对（$(hash_of "$target")）；加 --force 删掉重下，或自己核对那个文件"
  fi
  rm -f "$target"

  part="$target.part"
  say "下载  $MODEL_SUBDIR/${name}（$want_bytes 字节）"
  if [ -f "$part" ]; then
    curl -fL --retry 3 --connect-timeout 30 -C - -o "$part" "$BASE_URL/$name" ||
      die "下载失败：$BASE_URL/${name}（半截文件留在 ${part}，重跑会续传）"
  else
    curl -fL --retry 3 --connect-timeout 30 -o "$part" "$BASE_URL/$name" ||
      die "下载失败：$BASE_URL/$name"
  fi
  got="$(hash_of "$part")"
  if [ "$got" != "$want_sha" ]; then
    die "$name 下载回来的 sha256 不对：实际 ${got}（$(bytes_of "$part") 字节），期望 ${want_sha}（$want_bytes 字节）；半截文件 $part 已保留，先删掉它再重跑"
  fi
  mv "$part" "$target"
  say "ok    $MODEL_SUBDIR/$name  $got"
done

if [ "$WEIGHTS_ONLY" -eq 1 ]; then
  record_manifest
  say "只取了权重：$DEST/$MODEL_SUBDIR"
  say "metallib 还没有来源。之后补：code/scripts/fetch-model.sh --dest \"$DEST\" --metallib-from-build"
  exit 0
fi

# ---- metallib ----
if [ -n "$METALLIB_SRC" ] && [ "$FROM_BUILD" -eq 1 ]; then
  die "--metallib 与 --metallib-from-build 只能选一个"
fi

if [ "$FROM_BUILD" -eq 1 ]; then
  found=""
  found="$(ls -t "$ROOT"/code/voice-engine/target/*/*/build/mlx-sys-*/out/build/lib/mlx.metallib 2>/dev/null | head -1 || true)"
  [ -n "$found" ] || die "本机构建目录里没有 mlx.metallib。先编一次带 metal 的归档：
  cd code/voice-engine && export SDKROOT=\"\$(xcrun --sdk macosx --show-sdk-path)\" &&
  export LIBRARY_PATH=\"\$SDKROOT/usr/lib\" MLX_SYS_SYSTEM_SDK=1 &&
  cargo build -p voice-ffi --features metal --target aarch64-apple-darwin
（见 code/voice-engine/BUILD.txt）"
  METALLIB_SRC="$found"
  say "取自构建目录：$METALLIB_SRC"
fi

if [ -z "$METALLIB_SRC" ]; then
  if [ -f "$DEST/mlx.metallib" ] && [ "$(hash_of "$DEST/mlx.metallib")" = "$METALLIB_SHA256" ]; then
    say "已有  mlx.metallib"
  else
    die "没有 mlx.metallib 的来源。这条给三种办法（Hugging Face 上没有 metallib 镜像，脚本不会去猜）：
  1) 本机编过 metal 归档：--metallib-from-build
  2) 手上有一份：--metallib <路径>（或环境变量 VOICE_METALLIB）
  3) 只想先取权重：现在这个目录已经拿到了 $MODEL_SUBDIR/，metallib 之后再补"
  fi
else
  if [ ! -f "$METALLIB_SRC" ]; then
    die "metallib 不存在：$METALLIB_SRC"
  fi
  if [ -L "$METALLIB_SRC" ]; then
    die "metallib 是符号链接：${METALLIB_SRC}；请指向真实文件"
  fi
  got="$(hash_of "$METALLIB_SRC")"
  if [ "$got" != "$METALLIB_SHA256" ]; then
    if [ "$ACCEPT_NEW_METALLIB" -eq 1 ]; then
      say "注意  mlx.metallib 是另一次编译的产物：sha256 ${got}（脚本记录 ${METALLIB_SHA256}）；已按 --accept-new-metallib 接受"
      METALLIB_BYTES="$(bytes_of "$METALLIB_SRC")"
    else
      die "mlx.metallib 的 sha256 与脚本记录的不同：实际 ${got}（$(bytes_of "$METALLIB_SRC") 字节），记录 ${METALLIB_SHA256}（$METALLIB_BYTES 字节）。
这份是 MLX 内核的编译产物，换 Xcode 或换机器会变。确认它来自新机器上的 cargo build 之后，加 --accept-new-metallib 重跑（脚本会把新的哈希写进 MANIFEST.sha256）。"
    fi
  fi
  if [ "$(hash_of "$DEST/mlx.metallib" 2>/dev/null || true)" != "$(hash_of "$METALLIB_SRC")" ]; then
    rm -f "$DEST/mlx.metallib"
    cp "$METALLIB_SRC" "$DEST/mlx.metallib"
  fi
  check_one "$DEST/mlx.metallib" "$(hash_of "$METALLIB_SRC")" "$(bytes_of "$METALLIB_SRC")" "mlx.metallib"
  say "ok    mlx.metallib  $(hash_of "$DEST/mlx.metallib")"
fi

record_manifest
say ""
say "资产目录：$DEST"
say "装包：MODEL_DIR=\"$DEST/$MODEL_SUBDIR\" METALLIB=\"$DEST/mlx.metallib\" code/Client/scripts/make-app.sh"