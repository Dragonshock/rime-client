# 038-gap-voice-implementation-remaining：按 037 写代码时仍然会挡住的未知

本文只记录仍然未知、并且会挡住按 `037` 写代码的问题。有则每条写出未知、存在依据、现在是否需要补证。`033` 记下的五项，以及源码已经写明的调用，不在这里重开。`031` 仍是未确认蓝图，本文不改它。

用户原话：「我决定以你的方案、决策和思路进行开发。仍未确认蓝图，请自行调研填补空白未知，不再交由我来拍板。请帮我构建 workflow 进行详细工程分析，落地文档，基于调研结果帮我规划一份实施方案。我的目标是：在这个 Rime 输入法客户端里实现上述语音接口。」

`036` 在 `dev-docs/notes/` 没有落地文件。`037` 写明交给该文的异议清单是空的，没有条目要吸收。

## 结论

没有此类缺口。

按 `037` 写代码时要落笔的顺序、参数、记录字段和拒绝条件，都写在该文里，或写在它点名的控制器、`finish_streaming` 与已安装 `mlx-qwen3-asr==0.4.4` 的 `streaming.py` 上。`037` 标成源码未写、并且写明不拿来当条件的屏幕结果、替换范围、失焦入口、尾块字符差、收尾时延，以及替换文本是否错误更少，都不进入这些调用。缺这几项答案，仍然按该文的当前方案写。

## 检索日志

| # | 动作 | 地址或查询 | 结果 |
| --- | --- | --- | --- |
| 1 | 本地阅读 | `032`、`033`、`034`、`035`、`037` | 五项已写进 `037` 的接口；`034`、`035` 补上的是缓存、普通键与收尾函数 |
| 2 | 查找 `036` | `dev-docs/notes/` | 没有 `036` 文件。与 `037` 的「没有落地文件」一致 |
| 3 | 读修饰键 | `dev-docs/refs/code/squirrel` 的 `SquirrelInputController.swift` `handle`，`MacOSKeyCodes.swift` 的 `modifierKeycodes` | 左右修饰键码与 `flagsChanged` 路径已写明。`037` 的取走规则接在 `processKey` 之前 |
| 4 | 读识别函数 | `/tmp/gap024/venv` 内 `mlx-qwen3-asr==0.4.4` 的 `streaming.py`：`init_streaming`、`feed_audio`、`_sanitize_stream_pcm` | 形参与 `037` 所写的一组一致。整数 PCM 在 `_sanitize_stream_pcm` 里按整型宽度缩成 float32 |
| 5 | 路径检查 | `dev-docs/notes/038-gap-voice-implementation-remaining.md` | 写入前不存在 |

源码已经写明上述调用。本次没有调用 REA。

<!-- gap-complete -->
