#!/usr/bin/env python3
"""对 mlx_qwen3_asr 参考实现做前向，逐层导出可对照的中间量。

参考实现装在 `code/voice-worker/.venv` 里，只作对照标准，不随安装包走；本脚本
也不被产品路径调用。导出的每一项都是 f32 小端 `.bin`，形状与来源函数名写进
`assets/manifest.json`。

一共导出六项：

    mel                      (128, n_frames)   对数梅尔谱（参考 f32 值）
    conv_stem_out            (16, 13, 480)     卷积茎对第 0 个 100 帧块的输出（NHWC）；
                                               多块模式下为 (整块数, 16, 13, 480)
    encoder_layer0_out       (1, n_tokens, 896) 第 0 层编码器的残差输出
    encoder_proj_out         (n_tokens, 1024)  编码器末尾 ln_post/gelu/proj1/proj2
    prefill_logits_64        (64,)             预填充最后一个位置的前 64 维 logits
    greedy_tokens_3          (3,)              贪心解码的前 3 个 token（f32 存整数）

拿不到的项不伪造：某项抓不到时，它的 key 与原因写进 manifest 的 `unavailable`
数组，不写假张量；其余项照常导出。

用法：

    code/voice-worker/.venv/bin/python code/voice-oracle/dump_reference.py
    code/voice-worker/.venv/bin/python code/voice-oracle/dump_reference.py --wav /tmp/a.wav
    code/voice-worker/.venv/bin/python code/voice-oracle/dump_reference.py --pcm /tmp/a.bin --out /tmp/ref
    code/voice-worker/.venv/bin/python code/voice-oracle/dump_reference.py --multichunk

不带参数时用内置确定性探针（16 kHz 单声道 i16，32000 采样，公式
`sample = (i * 37) % 2001 - 1000`）。带 `--wav`/`--pcm` 时对给定音频导出同一组量，
资产与 manifest 写到 `--out` 指定目录（默认仍是 assets 目录）。
`--multichunk` 对 `assets/zh` 下的 `s1_3.0.pcm`、`s1_4.0.pcm` 各导一套（3 / 4 个整块），
每段写到 `assets/multichunk/<段名>/`，并在 `assets/multichunk/manifest.json` 写汇总；
这条路径不碰 assets 根下的 200 帧资产，且卷积茎落整批（第 0 维是块号）。
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import wave
from pathlib import Path

import numpy as np

SAMPLE_RATE = 16000
PROBE_SAMPLES = 32000
PROBE_FORMULA = "sample = (i * 37) % 2001 - 1000"

ASSETS_DIR = Path(__file__).resolve().parent / "assets"
# 多块资产：`assets/zh` 下的真实中文语音；3.0 s / 4.0 s 是恰好 3 / 4 个整块。
SEGMENTS_DIR = ASSETS_DIR / "zh"
MULTICHUNK_SEGMENTS = ("s1_3.0", "s1_4.0")
DEFAULT_MODEL_GLOB = (
    "~/.cache/huggingface/hub/models--moona3k--mlx-qwen3-asr-0.6b-4bit/snapshots/*"
)

# 导出的 token 数（贪心解码前几个 token）
GREEDY_TOKENS = 3
# 预填充最后一个位置取多少维 logits
PREFILL_LOGITS_DIMS = 64


class CaptureFailed(Exception):
    """整体前向无法进行：例如参考模型没有编码器层，连一次前向都跑不起来。"""


# --------------------------------------------------------------------------
# 输入音频
# --------------------------------------------------------------------------


def build_probe_pcm(n_samples: int = PROBE_SAMPLES) -> np.ndarray:
    """生成确定性探针 PCM：i16 小端，16 kHz 单声道。"""
    i = np.arange(n_samples, dtype=np.int64)
    return (((i * 37) % 2001) - 1000).astype(np.int16)


def read_wav_pcm(path: Path) -> np.ndarray:
    """读未压缩 PCM wav，要求 16 kHz 单声道 16 位，不做重采样。"""
    with wave.open(str(path), "rb") as handle:
        channels = handle.getnchannels()
        sampwidth = handle.getsampwidth()
        rate = handle.getframerate()
        frames = handle.getnframes()
        raw = handle.readframes(frames)
    if sampwidth != 2:
        raise SystemExit(f"{path}: 只接受 16 位 PCM，实际 {sampwidth * 8} 位")
    if channels != 1:
        raise SystemExit(f"{path}: 只接受单声道，实际 {channels} 声道")
    if rate != SAMPLE_RATE:
        raise SystemExit(f"{path}: 只接受 {SAMPLE_RATE} Hz，实际 {rate} Hz（不重采样）")
    return np.frombuffer(raw, dtype="<i2").copy()


def read_raw_pcm(path: Path) -> np.ndarray:
    """读裸 i16 小端 PCM（16 kHz 单声道）。"""
    data = path.read_bytes()
    if len(data) % 2 != 0:
        raise SystemExit(f"{path}: 字节数 {len(data)} 不是偶数，不是 i16 PCM")
    return np.frombuffer(data, dtype="<i2").copy()


def pcm_to_float(pcm: np.ndarray) -> np.ndarray:
    """与 mlx_qwen3_asr.load_audio 的整数 PCM 路径一致：除以 32768。"""
    return pcm.astype(np.float32) / 32768.0


# --------------------------------------------------------------------------
# 资产写出
# --------------------------------------------------------------------------


def write_f32(path: Path, array: np.ndarray) -> None:
    """写出 f32 小端裸张量。"""
    path.write_bytes(np.ascontiguousarray(array, dtype="<f4").tobytes())


def write_i16(path: Path, array: np.ndarray) -> None:
    """写出 i16 小端裸 PCM。"""
    path.write_bytes(np.ascontiguousarray(array, dtype="<i2").tobytes())


# --------------------------------------------------------------------------
# 参考实现
# --------------------------------------------------------------------------


def resolve_model_dir(explicit: str | None) -> Path:
    """定位模型目录：命令行 > 环境变量 VOICE_MODEL_DIR > HF 缓存快照。"""
    if explicit:
        return Path(explicit).expanduser().resolve()
    env = os.environ.get("VOICE_MODEL_DIR")
    if env:
        return Path(env).expanduser().resolve()
    candidates = sorted(
        Path(DEFAULT_MODEL_GLOB).expanduser().parent.glob("*")
    )
    for candidate in candidates:
        if (candidate / "config.json").exists():
            return candidate
    raise SystemExit(
        "找不到模型目录：请用 --model 指定，或设置 VOICE_MODEL_DIR，"
        f"或把快照放到 {Path(DEFAULT_MODEL_GLOB).expanduser()}"
    )


def load_reference(model_dir: Path, dtype_name: str):
    """加载参考模型与分词器。"""
    import mlx.core as mx
    from mlx_qwen3_asr import load_model
    from mlx_qwen3_asr.tokenizer import Tokenizer

    dtype = mx.float16 if dtype_name == "float16" else mx.float32
    model, config = load_model(str(model_dir), dtype=dtype)
    tokenizer = Tokenizer(str(model_dir))
    return model, config, tokenizer, dtype


def capture_forward(model, config, mel, feature_lens, dtype):
    """跑一次编码器前向，同时抓卷积茎与第 0 层的输出。

    卷积茎通过替换 `_apply_conv_stem` 实例方法抓，第 0 层通过替换
    `AudioEncoderLayer.__call__` 类方法抓（Python 的 `layer(x)` 走类型上的
    `__call__`，实例属性拦不住）。抓完立刻还原。
    """
    import mlx.core as mx
    from mlx_qwen3_asr.encoder import AudioEncoderLayer

    captured: dict[str, object] = {}
    failures: list[dict] = []
    tower = model.audio_tower
    layers = list(tower.layers)
    if not layers:
        raise CaptureFailed("audio_tower.layers 为空，没有第 0 层可抓")

    if hasattr(tower, "_apply_conv_stem"):
        original_stem = tower._apply_conv_stem

        def patched_stem(x):
            out = original_stem(x)
            # `x` 形状 (n_chunks, 128, 100, 1)，按块批处理；每次调用都留一份，
            # 尾块会走第二次调用（宽度不足一块），形状与整块那次不同。
            captured.setdefault("conv_stem_calls", []).append(  # type: ignore[union-attr]
                (np.array(out), tuple(out.shape))
            )
            return out

        tower._apply_conv_stem = patched_stem  # type: ignore[method-assign]
    else:
        original_stem = None
        failures.append(
            {
                "key": "conv_stem_out",
                "reason": "AudioEncoder 没有 _apply_conv_stem 方法，卷积茎输出拿不到",
            }
        )

    layer0 = layers[0]
    original_layer_call = AudioEncoderLayer.__call__

    def patched_layer_call(self, x, mask=None):
        out = original_layer_call(self, x, mask=mask)
        if self is layer0:
            captured.setdefault("encoder_layer0_out_raw", np.array(out))
        return out

    AudioEncoderLayer.__call__ = patched_layer_call  # type: ignore[method-assign]
    try:
        audio_features, output_lens = tower(mel.astype(dtype), feature_lens)
        mx.eval(audio_features)
    finally:
        AudioEncoderLayer.__call__ = original_layer_call  # type: ignore[method-assign]
        if original_stem is not None:
            tower._apply_conv_stem = original_stem  # type: ignore[method-assign]

    stem_calls = captured.get("conv_stem_calls") or []
    stem_chunk0 = None
    stem_all_chunks = None
    stem_full_shape = None
    stem_call_shapes: list[list[int]] = []
    if not stem_calls:
        failures.append(
            {
                "key": "conv_stem_out",
                "reason": "前向里没有调用 _apply_conv_stem，卷积茎输出为空",
            }
        )
    else:
        first_out, first_shape = stem_calls[0]
        stem_chunk0 = first_out[0]
        stem_all_chunks = first_out
        stem_full_shape = tuple(first_shape)
        stem_call_shapes = [list(shape) for _, shape in stem_calls]
    layer0_out = captured.get("encoder_layer0_out_raw")
    if layer0_out is None:
        failures.append(
            {
                "key": "encoder_layer0_out",
                "reason": "前向里 layers[0] 没有被调用，第 0 层输出为空",
            }
        )

    return {
        "audio_features": audio_features,
        "output_lens": output_lens,
        "total_tokens": int(audio_features.shape[-2]),
        "conv_stem_out": None if stem_chunk0 is None else np.asarray(stem_chunk0, dtype=np.float32),
        "conv_stem_all_chunks": (
            None if stem_all_chunks is None else np.asarray(stem_all_chunks, dtype=np.float32)
        ),
        "conv_stem_full_shape": None if stem_full_shape is None else list(stem_full_shape),
        "conv_stem_call_shapes": stem_call_shapes,
        "encoder_layer0_out": None if layer0_out is None else np.asarray(layer0_out, dtype=np.float32),
        "failures": failures,
    }


def build_position_ids(mx, seq_len: int) -> "object":
    """三个维度同值的 MRoPE 位置，形状 (1, 3, seq_len)，与 transcribe 一致。"""
    positions = mx.arange(seq_len)[None, :]
    return mx.stack([positions, positions, positions], axis=1)


def prefill_logits(model, input_ids, audio_features, position_ids):
    """预填充并取最后一个位置的 logits（全词表）。"""
    import mlx.core as mx

    cache = model.create_cache(max_seq_len=int(input_ids.shape[1]) + GREEDY_TOKENS)
    logits = model.prefill(
        input_ids=input_ids,
        audio_features=audio_features,
        position_ids=position_ids,
        cache=cache,
    )
    mx.eval(logits)
    return cache, logits


def greedy_first_tokens(model, input_ids, audio_features, position_ids, count: int):
    """按参考的贪心路径取前 count 个 token。

    与 `mlx_qwen3_asr.generate._sample`（temperature 为 0 时 argmax）和
    `_build_decode_positions` 的做法一致：预填充一次，之后逐步把上一个 token
    送回 `model.step`。
    """
    import mlx.core as mx

    seq_len = int(input_ids.shape[1])
    cache, logits = prefill_logits(model, input_ids, audio_features, position_ids)
    tokens = [int(mx.argmax(logits.reshape(-1)))]
    for step in range(1, count):
        base = mx.arange(seq_len + step - 1, seq_len + step, dtype=position_ids.dtype)
        next_positions = mx.stack([base, base, base], axis=0)[None, :, :]
        logits = model.step(
            input_ids=mx.array([[tokens[-1]]]),
            position_ids=next_positions,
            cache=cache,
            validate_input_ids=False,
        )
        mx.eval(logits)
        tokens.append(int(mx.argmax(logits.reshape(-1))))
    return tokens


def reference_generate_tokens(model, input_ids, audio_features, position_ids, count: int):
    """用参考自带的 generate_with_info 再跑一遍，作为交叉核对。"""
    from mlx_qwen3_asr.generate import GenerationConfig, generate_with_info

    result = generate_with_info(
        model=model,
        input_ids=input_ids,
        audio_features=audio_features,
        position_ids=position_ids,
        config=GenerationConfig(max_new_tokens=count, temperature=0.0),
    )
    return result.tokens, result.finish_reason


# --------------------------------------------------------------------------
# 主流程
# --------------------------------------------------------------------------


def dump(audio_pcm: np.ndarray, audio_source: str, out_dir: Path, model_dir: Path,
         dtype_name: str, language: str | None, context: str,
         stem_all_chunks: bool = False, runtime=None) -> dict:
    """导出六项参考量、写资产与 manifest，返回 manifest 字典。

    `stem_all_chunks` 为真时卷积茎落盘成整批（第 0 维是块号），用于多块音频；
    为假时与旧资产一致，只落第 0 块 `(16, 13, 480)`。`runtime` 可传入已加载的
    `(model, config, tokenizer, dtype)`，多段连续导出时免去重复加载。
    """
    import mlx.core as mx
    from mlx_qwen3_asr.audio import compute_features

    if runtime is None:
        model, config, tokenizer, dtype = load_reference(model_dir, dtype_name)
    else:
        model, config, tokenizer, dtype = runtime

    audio = pcm_to_float(audio_pcm)
    mel, feature_lens = compute_features(audio, SAMPLE_RATE)
    mx.eval(mel)

    mel_np = np.asarray(mel[0], dtype=np.float32)  # (128, n_frames)

    forward = capture_forward(model, config, mel, feature_lens, dtype)
    audio_features = forward["audio_features"]
    audio_features_np = np.asarray(audio_features[0], dtype=np.float32)
    n_audio_tokens = int(audio_features.shape[1])

    prompt_tokens = tokenizer.build_prompt_tokens(
        n_audio_tokens=n_audio_tokens,
        language=language,
        context=context,
    )
    input_ids = mx.array([prompt_tokens])
    position_ids = build_position_ids(mx, len(prompt_tokens))

    _, logits = prefill_logits(model, input_ids, audio_features, position_ids)
    logits_tail = np.asarray(logits[0, -1, :PREFILL_LOGITS_DIMS], dtype=np.float32)

    tokens = greedy_first_tokens(
        model, input_ids, audio_features, position_ids, GREEDY_TOKENS
    )
    reference_tokens, finish_reason = reference_generate_tokens(
        model, input_ids, audio_features, position_ids, GREEDY_TOKENS
    )

    out_dir.mkdir(parents=True, exist_ok=True)
    tensors: list[dict] = []
    unavailable: list[dict] = list(forward["failures"])

    def emit(
        key: str,
        tensor: np.ndarray,
        source: str,
        note: str | None = None,
        extra: dict | None = None,
    ) -> None:
        array = np.asarray(tensor, dtype=np.float32)
        name = f"{key}.bin"
        write_f32(out_dir / name, array)
        entry = {
            "key": key,
            "file": name,
            "shape": list(array.shape),
            "dtype": "float32",
            "bytes": int(array.size * 4),
            "source": source,
        }
        if note:
            entry["note"] = note
        if extra:
            entry.update(extra)
        tensors.append(entry)

    emit(
        "mel",
        mel_np,
        "mlx_qwen3_asr.audio.log_mel_spectrogram",
        "参考以 f32 计算后转模型 dtype 送进编码器；这里存转换前的 f32 值",
    )
    if forward["conv_stem_out"] is not None:
        if stem_all_chunks and forward.get("conv_stem_all_chunks") is not None:
            emit(
                "conv_stem_out",
                forward["conv_stem_all_chunks"],
                "mlx_qwen3_asr.encoder.AudioEncoder._apply_conv_stem",
                "存的是前向里第一次 _apply_conv_stem 调用的整批输出，第 0 维是整块号"
                "（每块 100 梅尔帧），布局为参考的 NHWC (块, 频域 16, 时域 13, 480)；"
                "送进 conv_out 前参考会按块 transpose 成 (13, 480*16) 再拼起来。"
                "取本张量的 [0] 就是旧 200 帧资产里的 (16, 13, 480)",
                extra={
                    "captured_from": "首次 _apply_conv_stem 调用的完整输出",
                    "full_call_output_shape": forward["conv_stem_full_shape"],
                    "all_stem_call_shapes": forward["conv_stem_call_shapes"],
                    "chunks_in_first_call": forward["conv_stem_full_shape"][0],
                },
            )
        else:
            emit(
                "conv_stem_out",
                forward["conv_stem_out"],
                "mlx_qwen3_asr.encoder.AudioEncoder._apply_conv_stem",
                "存的是前向里第一次 _apply_conv_stem 调用的第 0 块，布局为参考的 NHWC "
                "(频域 16, 时域 13, 480 通道)；送进 conv_out 前参考会 transpose 成 (13, 480*16)",
                extra={
                    "captured_from": "首次 _apply_conv_stem 调用的输出[0]",
                    "full_call_output_shape": forward["conv_stem_full_shape"],
                    "all_stem_call_shapes": forward["conv_stem_call_shapes"],
                },
            )
    if forward["encoder_layer0_out"] is not None:
        layer0_out = forward["encoder_layer0_out"]
        covers_all = int(layer0_out.shape[1]) == forward["total_tokens"]
        note = (
            "第 0 层含残差的输出，送入第 1 层；本段音频只有一个注意力窗口，mask 为 None"
            if covers_all
            else "第 0 层含残差的输出；窗口化注意力下这一层按窗口分次调用，"
            "这里只留第一次调用（窗口 0）的片段"
        )
        emit(
            "encoder_layer0_out",
            layer0_out,
            "mlx_qwen3_asr.encoder.AudioEncoderLayer.__call__ (layers[0])",
            note,
            extra={
                "covers_full_sequence": covers_all,
                "total_encoder_tokens": forward["total_tokens"],
            },
        )
    emit(
        "encoder_proj_out",
        audio_features_np,
        "mlx_qwen3_asr.encoder.AudioEncoder._encode_single -> ln_post/gelu(proj1)/proj2",
        "编码器末尾投影（音频特征），也是注入文本嵌入的张量",
    )
    emit(
        "prefill_logits_64",
        logits_tail,
        "mlx_qwen3_asr.model.Qwen3ASRModel.prefill -> lm_head[:, -1:, :64]",
        "最后一个提示位置的 logits，只落前 64 维（全词表 151936 维）",
    )
    emit(
        "greedy_tokens_3",
        np.asarray(tokens, dtype=np.float32),
        "mlx_qwen3_asr.generate._sample(argmax) + Qwen3ASRModel.prefill/step",
        "贪心解码前 3 个 token，用 f32 存整数；参考 generate 会剥掉末尾 EOS",
    )

    pcm_file = "probe_pcm.bin" if audio_source == "probe" else "input_pcm.bin"
    probe_pcm_path = out_dir / pcm_file

    manifest = {
        "generated_by": "code/voice-oracle/dump_reference.py",
        "reference_package": "mlx_qwen3_asr 0.4.4（code/voice-worker/.venv，只作对照）",
        "model_dir": str(model_dir),
        "model_dtype": dtype_name,
        "activation_dtype": str(dtype),
        "sample_rate": SAMPLE_RATE,
        "conv_stem_layout": "all_chunks" if stem_all_chunks else "chunk0_only",
        "audio_input": {
            "source": audio_source,
            "file": probe_pcm_path.name,
            "dtype": "int16",
            "shape": [int(audio_pcm.size)],
            "bytes": int(audio_pcm.size * 2),
            "formula": PROBE_FORMULA if audio_source == "probe" else None,
            "to_float": "pcm.astype(f32) / 32768（与 mlx_qwen3_asr.load_audio 一致）",
        },
        "audio_frames": {
            "mel_frames": int(mel_np.shape[1]),
            "n_audio_tokens": n_audio_tokens,
            "encoder_output_lens": [int(v) for v in np.asarray(forward["output_lens"]).ravel()],
        },
        "prompt": {
            "language": language,
            "context": context,
            "length": len(prompt_tokens),
            "tokens": [int(t) for t in prompt_tokens],
        },
        "greedy": {
            "tokens": [int(t) for t in tokens],
            "text_from_tokens": tokenizer.decode([int(t) for t in tokens]),
            "reference_generate_tokens": [int(t) for t in reference_tokens],
            "reference_finish_reason": finish_reason,
            "matches_reference_generate": (
                [int(t) for t in tokens][: len(reference_tokens)] == [int(t) for t in reference_tokens]
            ),
        },
        "tensors": tensors,
        "unavailable": unavailable,
    }

    write_i16(probe_pcm_path, audio_pcm)
    (out_dir / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    return manifest


def dump_segments(segment_names: list[str], segments_dir: Path, out_dir: Path,
                 model_dir: Path, dtype_name: str, language: str | None,
                 context: str) -> dict:
    """对多段音频各导出一套参考量，并在 `out_dir` 写一份汇总 manifest。

    每段落到 `out_dir/<段名>/`，自己带一份扁平 manifest；汇总 manifest 把两段的
    熵（key、形状、dtype、来源函数）与音频 token 数收到一起。模型只加载一次。
    卷积茎在这些段上落整批（第 0 维是块号），因为多块差异正是要看的东西。
    """
    out_dir.mkdir(parents=True, exist_ok=True)
    runtime = load_reference(model_dir, dtype_name)

    segments: list[dict] = []
    unavailable: list[dict] = []
    audio_tokens: dict[str, int] = {}
    for name in segment_names:
        pcm_path = segments_dir / f"{name}.pcm"
        if not pcm_path.exists():
            raise SystemExit(f"找不到音频 {pcm_path}")
        pcm = read_raw_pcm(pcm_path)
        manifest = dump(
            audio_pcm=pcm,
            audio_source=f"pcm:{pcm_path}",
            out_dir=out_dir / name,
            model_dir=model_dir,
            dtype_name=dtype_name,
            language=language,
            context=context,
            stem_all_chunks=True,
            runtime=runtime,
        )
        n_tokens = int(manifest["audio_frames"]["n_audio_tokens"])
        audio_tokens[name] = n_tokens
        segments.append(
            {
                "segment": name,
                "dir": name,
                "manifest": f"{name}/manifest.json",
                "audio": manifest["audio_input"],
                "audio_frames": manifest["audio_frames"],
                "greedy": manifest["greedy"],
                "tensors": [
                    {**entry, "file": f"{name}/{entry['file']}"}
                    for entry in manifest["tensors"]
                ],
            }
        )
        unavailable.extend({"segment": name, **entry} for entry in manifest["unavailable"])

    index = {
        "generated_by": "code/voice-oracle/dump_reference.py --multichunk",
        "reference_package": "mlx_qwen3_asr 0.4.4（code/voice-worker/.venv，只作对照）",
        "purpose": "多个整块的音频上的逐层参考量；旧的 200 样点探针资产未动",
        "model_dir": str(model_dir),
        "model_dtype": dtype_name,
        "sample_rate": SAMPLE_RATE,
        "chunk_frames": 100,
        "audio_tokens": audio_tokens,
        "segments": segments,
        "unavailable": unavailable,
    }
    (out_dir / "manifest.json").write_text(
        json.dumps(index, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    return index


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        description="导出 mlx_qwen3_asr 的逐层参考量（只作对照，不进产品路径）"
    )
    source = parser.add_mutually_exclusive_group()
    source.add_argument("--wav", help="16 kHz 单声道 16 位 wav；不给则用内置探针")
    source.add_argument("--pcm", help="裸 i16 小端 16 kHz 单声道 PCM；不给则用内置探针")
    parser.add_argument("--out", help="资产目录，默认 code/voice-oracle/assets")
    parser.add_argument("--model", help="模型目录，默认 $VOICE_MODEL_DIR 或 HF 缓存快照")
    parser.add_argument(
        "--dtype", choices=["float16", "float32"], default="float16",
        help="参考模型激活 dtype，默认 float16（load_model 的默认值）",
    )
    parser.add_argument(
        "--language", default="Chinese",
        help='给提示尾部强制语言，传 "none" 表示不写（默认 Chinese，与 voice-worker 一致）',
    )
    parser.add_argument("--context", default="", help="系统提示里的上下文串，默认空")
    parser.add_argument(
        "--multichunk", action="store_true",
        help="对多段音频（默认 s1_3.0、s1_4.0）各导出一套量；默认为 assets/multichunk",
    )
    parser.add_argument(
        "--segments", default=",".join(MULTICHUNK_SEGMENTS),
        help="--multichunk 要导的段名，逗号分隔，相对 --segments-dir",
    )
    parser.add_argument(
        "--segments-dir", default=str(SEGMENTS_DIR),
        help="--multichunk 的音频目录，默认 code/voice-oracle/assets/zh",
    )
    args = parser.parse_args(argv)

    language = None if args.language.strip().lower() in {"", "none", "null"} else args.language
    model_dir = resolve_model_dir(args.model)

    if args.multichunk:
        if args.wav or args.pcm:
            raise SystemExit("--multichunk 不接受 --wav/--pcm，请用 --segments/--segments-dir")
        names = [item.strip() for item in args.segments.split(",") if item.strip()]
        if not names:
            raise SystemExit("--segments 为空")
        out_dir = Path(args.out).expanduser() if args.out else ASSETS_DIR / "multichunk"
        index = dump_segments(
            names,
            Path(args.segments_dir).expanduser(),
            out_dir,
            model_dir,
            args.dtype,
            language,
            args.context,
        )
        print(f"资产目录 {out_dir}（模型 {model_dir}）")
        for entry in index["segments"]:
            frames = entry["audio_frames"]
            samples = entry["audio"]["shape"][0]
            print(
                f"  {entry['segment']:<10} {samples:>6} 采样 / {samples / SAMPLE_RATE:.3f} s "
                f"梅尔 {frames['mel_frames']:>3} 帧 → 音频 token {frames['n_audio_tokens']:>3}"
            )
            for tensor in entry["tensors"]:
                print(f"      {tensor['key']:<20} {tuple(tensor['shape'])!s:<20} {tensor['file']}")
            print(f"      贪心前 {GREEDY_TOKENS} 个 token：{entry['greedy']['tokens']}")
        for entry in index["unavailable"]:
            print(f"  unavailable {entry['segment']}/{entry['key']}：{entry['reason']}")
        tokens = index["audio_tokens"]
        print("音频 token 数：" + "，".join(f"{k} = {v}" for k, v in tokens.items()))
        return 0

    if args.wav:
        pcm = read_wav_pcm(Path(args.wav).expanduser())
        audio_source = f"wav:{Path(args.wav).expanduser()}"
    elif args.pcm:
        pcm = read_raw_pcm(Path(args.pcm).expanduser())
        audio_source = f"pcm:{Path(args.pcm).expanduser()}"
    else:
        pcm = build_probe_pcm()
        audio_source = "probe"

    out_dir = Path(args.out).expanduser() if args.out else ASSETS_DIR

    manifest = dump(
        audio_pcm=pcm,
        audio_source=audio_source,
        out_dir=out_dir,
        model_dir=model_dir,
        dtype_name=args.dtype,
        language=language,
        context=args.context,
    )

    print(f"音频来源 {audio_source}，{pcm.size} 采样 / {pcm.size / SAMPLE_RATE:.3f} s")
    print(f"资产目录 {out_dir}")
    for entry in manifest["tensors"]:
        print(f"  {entry['key']:<20} {tuple(entry['shape'])!s:<22} {entry['bytes']:>10} 字节")
    for entry in manifest["unavailable"]:
        print(f"  unavailable {entry['key']}：{entry['reason']}")
    print(f"贪心前 {GREEDY_TOKENS} 个 token：{manifest['greedy']['tokens']}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
