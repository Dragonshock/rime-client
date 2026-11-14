#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
CLIENT="$ROOT/code/Client"
BIN="$CLIENT/.build/out/Products/Debug/Squirrel"
APP="$CLIENT/dist/Squirrel.app"
HEADERS_OK="$CLIENT/Headers/rime/key_table.h"

test -x "$BIN"
test -f "$HEADERS_OK"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Frameworks" "$APP/Contents/Resources"

cp "$BIN" "$APP/Contents/MacOS/Squirrel"
cp "$ROOT/dev-docs/refs/code/squirrel/resources/rime.pdf" "$APP/Contents/Resources/rime.pdf"
cp "$ROOT/code/voice-worker/worker.py" "$APP/Contents/Resources/worker.py"

# mlx.metallib 来自 mlx-sys 的构建输出。权重来自本机 Hugging Face 快照，必须解开符号链接。
# 构建目录里可能有多份 mlx-sys 产物，只取最新的一份。
METALLIB="$(find "$ROOT/code/voice-engine/target" -path '*/mlx-sys-*/out/build/lib/mlx.metallib' -type f -print0 | xargs -0 ls -t 2>/dev/null | head -1)"
test -n "$METALLIB"
test -f "$METALLIB"
test ! -L "$METALLIB"
cp -L "$METALLIB" "$APP/Contents/Resources/mlx.metallib"
test -f "$APP/Contents/Resources/mlx.metallib"
test ! -L "$APP/Contents/Resources/mlx.metallib"

HF_ROOT="$HOME/.cache/huggingface/hub/models--moona3k--mlx-qwen3-asr-0.6b-4bit"
REV="$(tr -d '[:space:]' < "$HF_ROOT/refs/main")"
SNAP="$HF_ROOT/snapshots/$REV"
test -f "$SNAP/weights.safetensors"
test -f "$SNAP/config.json"
test -f "$SNAP/tokenizer_config.json"
test -f "$SNAP/vocab.json"
test -f "$SNAP/merges.txt"
MODEL="$APP/Contents/Resources/Models/Qwen3-ASR-0.6B-4bit"
mkdir -p "$MODEL"
cp -RL "$SNAP/." "$MODEL/"
test -f "$MODEL/weights.safetensors"
test -f "$MODEL/config.json"
test -f "$MODEL/tokenizer_config.json"
test -f "$MODEL/vocab.json"
test -f "$MODEL/merges.txt"
test ! -L "$MODEL/weights.safetensors"
test ! -L "$MODEL/config.json"
test ! -L "$MODEL/tokenizer_config.json"
test ! -L "$MODEL/vocab.json"
test ! -L "$MODEL/merges.txt"
if find "$MODEL" -type l | grep -q .; then
  echo "model directory still contains a symlink" >&2
  exit 1
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
