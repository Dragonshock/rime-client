#!/bin/bash
# 打本机包。模型与 metallib 显式给出，不再用 find 到构建目录里猜。
#
# 用法（都可省，省了用下面的默认值）
#   code/Client/scripts/make-app.sh
#   VOICE_MODEL_DIR=/tmp/voice/qwen3-asr-0.6b-4bit VOICE_METALLIB=/tmp/voice/mlx.metallib \
#     code/Client/scripts/make-app.sh
#   code/Client/scripts/make-app.sh --model-dir DIR --metallib FILE --app /tmp/X.app
#
# 默认值
#   VOICE_MODEL_DIR = $ROOT/code/assets/qwen3-asr-0.6b-4bit
#   VOICE_METALLIB  = $ROOT/code/assets/mlx.metallib
#   OUT_APP         = $ROOT/code/Client/dist/Squirrel.app
# 这两份资产由 code/scripts/fetch-model.sh 取到 code/assets，不进 git。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
CLIENT="$ROOT/code/Client"
BIN="$CLIENT/.build/out/Products/Debug/Squirrel"
APP="${OUT_APP:-$CLIENT/dist/Squirrel.app}"
HEADERS_OK="$CLIENT/Headers/rime/key_table.h"
ASSETS="${VOICE_ASSETS:-$ROOT/code/assets}"
MODEL_DIR="${VOICE_MODEL_DIR:-$ASSETS/qwen3-asr-0.6b-4bit}"
METALLIB="${VOICE_METALLIB:-$ASSETS/mlx.metallib}"

fail() {
  printf 'make-app.sh: %s\n' "$*" >&2
  exit 1
}

usage() {
  sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'
  exit 0
}

while [ $# -gt 0 ]; do
  case "$1" in
    --model-dir)
      [ $# -ge 2 ] || fail "--model-dir 后面要给目录"
      MODEL_DIR="$2"
      shift 2
      ;;
    --metallib)
      [ $# -ge 2 ] || fail "--metallib 后面要给文件路径"
      METALLIB="$2"
      shift 2
      ;;
    --app)
      [ $# -ge 2 ] || fail "--app 后面要给包路径"
      APP="$2"
      shift 2
      ;;
    -h | --help)
      usage
      ;;
    *)
      fail "不认识的参数：$1（--help 看用法）"
      ;;
  esac
done

[ -x "$BIN" ] || fail "没有可执行文件：$BIN
先编：cd code/Client && swift build --product Squirrel"
[ -f "$HEADERS_OK" ] || fail "缺头文件：$HEADERS_OK（code/Client/Headers 不完整）"

# 资产先全部核对，再动 dist：这样缺文件时不会留下半个包。
[ -e "$METALLIB" ] || fail "没有 mlx.metallib：$METALLIB
取资产：code/scripts/fetch-model.sh --metallib-from-build
或者用 --metallib FILE（等价环境变量 VOICE_METALLIB）显式给出"
[ -f "$METALLIB" ] || fail "mlx.metallib 不是普通文件：$METALLIB"
[ -L "$METALLIB" ] && fail "mlx.metallib 是符号链接：$METALLIB；给真实文件（不要拷 HF 快照里的链接）"
[ -s "$METALLIB" ] || fail "mlx.metallib 是空文件：$METALLIB"

[ -d "$MODEL_DIR" ] || fail "没有模型目录：$MODEL_DIR
取资产：code/scripts/fetch-model.sh --dest "$ASSETS"
或者用 --model-dir DIR（等价环境变量 VOICE_MODEL_DIR）显式给出"
for f in config.json merges.txt quantization_config.json tokenizer_config.json vocab.json weights.safetensors; do
  [ -f "$MODEL_DIR/$f" ] || fail "模型目录缺文件：$MODEL_DIR/$f
完整清单见 code/scripts/fetch-model.sh 的注释；重取：code/scripts/fetch-model.sh --dest \"$ASSETS\""
  [ -L "$MODEL_DIR/$f" ] && fail "$MODEL_DIR/$f 是符号链接；包里的权重必须是真实文件"
  [ -s "$MODEL_DIR/$f" ] || fail "$MODEL_DIR/$f 是空文件"
done
if find "$MODEL_DIR" -type l | grep -q .; then
  fail "模型目录里还有符号链接：$(find "$MODEL_DIR" -type l | head -3 | tr '\n' ' ')"
fi

# 有清单就逐个核对，防止把改过的权重打进包。
if [ -f "$ASSETS/MANIFEST.sha256" ]; then
  ( cd "$ASSETS" && shasum -a 256 -c MANIFEST.sha256 --quiet ) ||
    fail "$ASSETS 里的文件与 MANIFEST.sha256 不一致；重取：code/scripts/fetch-model.sh --dest \"$ASSETS\" --force"
fi

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Frameworks" "$APP/Contents/Resources"

cp "$BIN" "$APP/Contents/MacOS/Squirrel"
cp "$ROOT/dev-docs/refs/code/squirrel/resources/rime.pdf" "$APP/Contents/Resources/rime.pdf"
cp "$ROOT/code/voice-worker/worker.py" "$APP/Contents/Resources/worker.py"

cp "$METALLIB" "$APP/Contents/Resources/mlx.metallib"
[ -f "$APP/Contents/Resources/mlx.metallib" ] || fail "metallib 没进包"
[ -L "$APP/Contents/Resources/mlx.metallib" ] && fail "包里的 metallib 是符号链接"

MODEL="$APP/Contents/Resources/Models/Qwen3-ASR-0.6B-4bit"
mkdir -p "$MODEL"
cp -R "$MODEL_DIR/." "$MODEL/"
for f in config.json merges.txt quantization_config.json tokenizer_config.json vocab.json weights.safetensors; do
  [ -f "$MODEL/$f" ] || fail "包里缺 $MODEL/$f"
  [ -L "$MODEL/$f" ] && fail "包里的 $MODEL/$f 是符号链接"
done
if find "$MODEL" -type l | grep -q .; then
  fail "包里的模型目录还有符号链接"
fi
if [ -f "$ASSETS/MANIFEST.sha256" ]; then
  cp "$ASSETS/MANIFEST.sha256" "$APP/Contents/Resources/voice-assets.sha256"
fi

mkdir -p "$APP/Contents/Resources/zh-Hans.lproj" "$APP/Contents/Resources/en.lproj"
cat > "$APP/Contents/Resources/zh-Hans.lproj/InfoPlist.strings" << 'EOF'
"im.dragonshock.inputmethod.RimeClient" = "RimeClient";
"im.dragonshock.inputmethod.RimeClient.Hans" = "RimeClient";
"im.dragonshock.inputmethod.RimeClient.Hant" = "RimeClient 繁体";
EOF
cat > "$APP/Contents/Resources/en.lproj/InfoPlist.strings" << 'EOF'
"im.dragonshock.inputmethod.RimeClient" = "RimeClient";
"im.dragonshock.inputmethod.RimeClient.Hans" = "RimeClient";
"im.dragonshock.inputmethod.RimeClient.Hant" = "RimeClient Traditional";
EOF
ditto "$CLIENT/.build/out/Products/Debug/Sparkle.framework" "$APP/Contents/Frameworks/Sparkle.framework"

python3 - "$BIN" "$APP" << 'PY'
import os, shutil, subprocess, sys
binary, app = sys.argv[1], sys.argv[2]
fw = os.path.join(app, "Contents", "Frameworks")

def deps(path):
    out = subprocess.check_output(["otool", "-L", path], text=True)
    libs = []
    for line in out.splitlines()[1:]:
        lib = line.strip().split(" (")[0]
        if lib.startswith("/usr/lib") or lib.startswith("/System"):
            continue
        if lib.startswith("@rpath/Sparkle"):
            continue
        libs.append(lib)
    return libs

needed = []
seen = set()
stack = deps(binary)
while stack:
    lib = stack.pop()
    if lib in seen or lib.startswith("@rpath/"):
        continue
    if not os.path.exists(lib):
        raise SystemExit(f"missing library: {lib}")
    seen.add(lib)
    needed.append(lib)
    stack.extend(deps(lib))

for lib in needed:
    dest = os.path.join(fw, os.path.basename(lib))
    shutil.copy2(lib, dest)
    subprocess.check_call(["install_name_tool", "-id", f"@rpath/{os.path.basename(lib)}", dest])

def retarget(path):
    for lib in deps(path):
        base = os.path.basename(lib)
        if os.path.exists(os.path.join(fw, base)):
            subprocess.check_call(["install_name_tool", "-change", lib, f"@rpath/{base}", path])

retarget(os.path.join(app, "Contents", "MacOS", "Squirrel"))
for name in os.listdir(fw):
    path = os.path.join(fw, name)
    if os.path.isfile(path) and name.endswith(".dylib"):
        retarget(path)
print("bundled", len(needed), "dylibs")
PY

install_name_tool -add_rpath @executable_path/../Frameworks "$APP/Contents/MacOS/Squirrel"

cat > "$APP/Contents/Info.plist" << 'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>zh-Hans</string>
	<key>CFBundleExecutable</key>
	<string>Squirrel</string>
	<key>CFBundleIdentifier</key>
	<string>im.dragonshock.inputmethod.RimeClient</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>RimeClient</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>1.0</string>
	<key>CFBundleVersion</key>
	<string>1</string>
	<key>CFBundleSignature</key>
	<string>????</string>
	<key>NSPrincipalClass</key>
	<string>NSApplication</string>
	<key>LSUIElement</key>
	<true/>
	<key>LSBackgroundOnly</key>
	<false/>
	<key>NSMicrophoneUsageDescription</key>
	<string>语音输入需要使用麦克风。</string>
	<key>InputMethodConnectionName</key>
	<string>RimeClient_Connection</string>
	<key>InputMethodServerControllerClass</key>
	<string>Squirrel.SquirrelInputController</string>
	<key>InputMethodServerDelegateClass</key>
	<string>Squirrel.SquirrelInputController</string>
	<key>TICapsLockLanguageSwitchCapable</key>
	<true/>
	<key>TISInputSourceID</key>
	<string>im.dragonshock.inputmethod.RimeClient</string>
	<key>ComponentInputModeDict</key>
	<dict>
		<key>tsInputModeListKey</key>
		<dict>
			<key>im.dragonshock.inputmethod.RimeClient.Hans</key>
			<dict>
				<key>TISInputSourceID</key>
				<string>im.dragonshock.inputmethod.RimeClient.Hans</string>
				<key>TISIntendedLanguage</key>
				<string>zh-Hans</string>
				<key>tsInputModeMenuIconFileKey</key>
				<string>rime.pdf</string>
				<key>tsInputModePaletteIconFileKey</key>
				<string>rime.pdf</string>
				<key>tsInputModeDefaultStateKey</key>
				<true/>
				<key>tsInputModeIsVisibleKey</key>
				<true/>
				<key>tsInputModeScriptKey</key>
				<string>smUnicodeScript</string>
			</dict>
			<key>im.dragonshock.inputmethod.RimeClient.Hant</key>
			<dict>
				<key>TISInputSourceID</key>
				<string>im.dragonshock.inputmethod.RimeClient.Hant</string>
				<key>TISIntendedLanguage</key>
				<string>zh-Hant</string>
				<key>tsInputModeMenuIconFileKey</key>
				<string>rime.pdf</string>
				<key>tsInputModePaletteIconFileKey</key>
				<string>rime.pdf</string>
				<key>tsInputModeDefaultStateKey</key>
				<false/>
				<key>tsInputModeIsVisibleKey</key>
				<true/>
				<key>tsInputModeScriptKey</key>
				<string>smUnicodeScript</string>
			</dict>
		</dict>
		<key>tsVisibleInputModeOrderedArrayKey</key>
		<array>
			<string>im.dragonshock.inputmethod.RimeClient.Hans</string>
			<string>im.dragonshock.inputmethod.RimeClient.Hant</string>
		</array>
	</dict>
</dict>
</plist>
EOF

cat > "$APP/Contents/Resources/entitlements.plist" << 'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>com.apple.security.app-sandbox</key>
	<false/>
	<key>com.apple.security.cs.disable-library-validation</key>
	<true/>
	<key>com.apple.security.network.client</key>
	<true/>
	<key>com.apple.security.device.audio-input</key>
	<true/>
</dict>
</plist>
EOF

codesign --force --sign - "$APP/Contents/Frameworks/"*.dylib
codesign --force --sign - "$APP/Contents/Frameworks/Sparkle.framework"
codesign --force --sign - --entitlements "$APP/Contents/Resources/entitlements.plist" "$APP"
echo "app: $APP"
