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
