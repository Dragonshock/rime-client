# RimeClient

macOS 输入法包。可执行文件链接自带的 `librime`，语音功能在按住右 Option 时采集麦克风，经本机 `mlx_qwen3_asr` 识别后，用输入法的 `setMarkedText` / `insertText` 写入当前文本框。

当前安装包：

`code/Client/dist/Squirrel.app`

已安装副本：

`~/Library/Input Methods/RimeClient.app`

包标识 `im.dragonshock.inputmethod.RimeClient`。菜单名是 **RimeClient**（简体）和 **RimeClient 繁体**。二进制是 Apple 芯片上的 `arm64`，工程平台是 macOS 14。

## 没有安装鼠须管时，能不能当 Rime 客户端用

可以启动，不依赖已经装好的鼠须管应用。`/Library/Input Methods/Squirrel.app` 不必存在。`librime.1.dylib` 以及它依赖的动态库放在这个包的 `Contents/Frameworks` 里，不读取 Nix 商店里的绝对路径。

它还不是一套自带方案的完整 Rime 配置。程序把共享数据目录设为包内 `Contents/Resources`，把用户数据目录设为 `~/Library/Rime`。当前包的 Resources 里没有 `default.yaml`、方案或词库，只有 `rime.pdf`、`worker.py` 和本地化字符串。因此：

- 这台电脑上的 `~/Library/Rime` 里已经有方案时，客户端和鼠须管使用同一份用户词库与方案，键盘输入可以沿用那份配置。
- 一台从来没有部署过 Rime 的电脑上，只有这个 `.app` 时，没有方案可编译，键盘侧打不出中文。鼠须管本身不是进程依赖，缺的是那份方案数据。

## 新装的客户端是否带语音输入

带。输入法进程里接了 `VoiceHandleGate`：文本框有焦点时，按住右 Option 开始采集，松开后把识别定稿经 `commit(string:)` 提交。说话过程中的部分文本走 `show(preedit:)`。不使用剪贴板，也不合成 Command-V。

语音识别不在输入法进程里运行。包内有 `Contents/Resources/worker.py`。解释器不在包内，而是按编译时的源文件路径，解析到构建机上的：

`code/voice-worker/.venv/bin/python3`

这个解释器里安装了 `mlx-qwen3-asr`。路径拼不出来时，程序不改用 `/usr/bin/python3`，语音不会开始。因此在本仓库所在的这台电脑上，语音入口是接上的；把 `.app` 单独拷到另一台电脑时，这个解释器路径不存在，语音不会启动。模型权重也不在安装包里，第一次成功调用时由该虚拟环境自己下载。

## 在一台新电脑上如何安装

下面是按当前这个包能够做到的安装。系统需要 Apple 芯片和 macOS 14 或更高版本。

1. 拷贝 `Squirrel.app` 到该用户的 `~/Library/Input Methods/`，并改名为 `RimeClient.app`。不要覆盖 `/Library/Input Methods/Squirrel.app`。
2. 在终端执行：

```sh
"$HOME/Library/Input Methods/RimeClient.app/Contents/MacOS/Squirrel" --register-input-source
"$HOME/Library/Input Methods/RimeClient.app/Contents/MacOS/Squirrel" --enable-input-source
```

3. 打开输入法菜单，选择 **RimeClient**。若菜单里暂时没有，先切走再打开一次菜单。
4. 键盘要能打出中文，还需要已有的 `~/Library/Rime` 方案数据。这个包不附带方案。
5. 语音只在构建机那条 `code/voice-worker/.venv/bin/python3` 路径仍然有效时可用。新电脑上仅拷贝 `.app` 不会带上这个虚拟环境。按住右 Option 之前，系统还会询问麦克风权限。

签名是本机临时签名，没有 Team ID。另一台电脑若拒绝打开，需要在系统设置里允许该包。

重新生成本地包：

```sh
cd code/Client
swift build --product Squirrel
./scripts/make-app.sh
```

## 从 GitHub Release 安装（给用户）

Release `v0.1.0` 有三个资产：输入法包本体、识别权重、键盘配置（白霜拼音）。安装一次即可正常打字与语音。

```sh
mkdir rimeclient && cd rimeclient
gh release download v0.1.0 --repo Dragonshock/rime-client
shasum -a 256 -c *.sha256

tar --zstd -xf RimeClient-20261115.tar.zst
tar --zstd -xf models-qwen3-asr-0.6b-4bit.tar.zst -C RimeClient-20261115/assets
mkdir -p RimeClient-20261115/rime && tar --zstd -xf rime-config.tar.zst -C RimeClient-20261115/rime

cd RimeClient-20261115
scripts/install-app.sh --assets "$PWD/assets" --rime-config "$PWD/rime"
```

`install-app.sh` 会：停掉在跑的旧进程 → 把权重与 `mlx.metallib` 补进包并重签 → 复制到 `~/Library/Input Methods/RimeClient.app` → 校验签名与包内资产 → **仅当 `~/Library/Rime` 为空时**播种键盘配置 → 触发一次部署编译词库（首次几分钟）。

要覆盖已有配置用 `--force-rime`，它会先把旧目录改名备份。

### 分发时踩过的三个坑

1. **`--build` 原来是空跑。** 它没有设置 `shared_data_dir` 与 `user_data_dir`，librime 走默认路径，返回 `true` 但什么都不生成。现在两个目录都显式设置，并在终端打印 `deploy finished <bool>`。
2. **`--assets` 与 `--rime-config` 默认路径**改成先看脚本旁边的 `RimeClient.app`，封装包里的布局才找得到。
3. **`export-rime-config.sh` 的 rsync 用了 `-RL`**，`-R` 是 `--relative` 不是递归，结果导出 0 个文件。改成 `-rL`。

### 尚未在本机验证的

- 真·全新用户目录的一次完整安装（本机 `~/Library/Rime` 已有配置，用 `RIME_USER_DIR` 覆盖口验证过播种与部署能生成 63 MB 的 `build/`，但没在真正的空家目录上走过一遍）。
- 另一台机器上 Gatekeeper 是否放行。包是本机签名（adhoc，无 Team ID），被拦时执行一次 `xattr -dr com.apple.quarantine ~/Library/Input\ Methods/RimeClient.app`。
