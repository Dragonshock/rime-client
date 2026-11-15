#!/usr/bin/env python3
"""辅助进程：按行读取 JSON，调用 mlx_qwen3_asr 的 init_streaming / feed_audio / finish_streaming。

解码不在输入法 handle 的返回栈上。本进程只回整段 text，不回 rewrite 计数。
"""

import base64
import json
import sys

MODEL = "moona3k/mlx-qwen3-asr-0.6b-4bit"
state = None
session_id = None


def reply(payload):
    sys.stdout.write(json.dumps(payload, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def fail(message, session=None):
    reply({"ok": False, "session": session, "message": message})


def handle_start(message):
    global state, session_id
    from mlx_qwen3_asr.streaming import init_streaming

    session_id = message.get("session")
    kwargs = {
        "model": message.get("model") or MODEL,
        "chunk_size_sec": message.get("chunk_size_sec", 2.0),
        "max_context_sec": message.get("max_context_sec", 30.0),
        "sample_rate": message.get("sample_rate", 16000),
        "language": message.get("language", "Chinese"),
    }
    if "context" in message and message["context"] is not None:
        kwargs["context"] = message["context"]
    state = init_streaming(**kwargs)
    reply({"ok": True, "session": session_id, "kind": "ready"})


def handle_feed(message):
    global state
    from mlx_qwen3_asr.streaming import feed_audio

    if state is None or message.get("session") != session_id:
        fail("session mismatch", message.get("session"))
        return
    raw = base64.b64decode(message.get("pcm_b64") or "")
    if not raw:
        reply({"ok": True, "session": session_id, "decoded": False, "text": state.text or ""})
        return
    import numpy as np

    pcm = np.frombuffer(raw, dtype="<i2")
    before = state.text or ""
    state = feed_audio(pcm, state)
    text = state.text or ""
    reply({
        "ok": True,
        "session": session_id,
        "decoded": text != before,
        "kind": "partial",
        "text": text,
    })


def handle_finish(message):
    global state
    from mlx_qwen3_asr.streaming import finish_streaming

    if state is None or message.get("session") != session_id:
        fail("session mismatch", message.get("session"))
        return
    state = finish_streaming(state)
    reply({
        "ok": True,
        "session": session_id,
        "kind": "final",
        "text": state.text or "",
    })


def handle_discard(message):
    """取消之后丢掉这一枚：这条路上没有引擎会话表，放掉本进程的 stream 状态就行。

    不回包：丢弃不是一次识别结果，回了反而让调用方多写一行 `rejected`。
    """
    global state, session_id
    if message.get("session") != session_id:
        return
    state = None
    session_id = None


def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            message = json.loads(line)
        except json.JSONDecodeError as exc:
            fail(str(exc))
            continue
        cmd = message.get("cmd")
        try:
            if cmd == "start":
                handle_start(message)
            elif cmd == "feed":
                handle_feed(message)
            elif cmd == "finish":
                handle_finish(message)
            elif cmd == "discard":
                handle_discard(message)
            elif cmd == "stop":
                reply({"ok": True, "kind": "stopped"})
                return
            else:
                fail("unknown cmd")
        except Exception as exc:
            fail(str(exc), message.get("session"))


if __name__ == "__main__":
    main()
