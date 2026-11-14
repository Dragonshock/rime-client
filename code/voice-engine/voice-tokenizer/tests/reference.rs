//! 与参考实现对照。模型目录由环境变量 `VOICE_MODEL_DIR` 给出，缺省时跳过。
//!
//! 参考值由 `code/voice-worker/.venv` 里的 `mlx_qwen3_asr` 分词器在同一目录上算出。

use std::path::PathBuf;

use voice_tokenizer::Tokenizer;

fn model_dir() -> Option<PathBuf> {
    let dir = std::env::var("VOICE_MODEL_DIR").ok()?;
    let path = PathBuf::from(dir);
    path.join("vocab.json").is_file().then_some(path)
}

#[test]
fn special_token_ids_match_reference() {
    let Some(dir) = model_dir() else {
        eprintln!("跳过：没有 VOICE_MODEL_DIR");
        return;
    };
    let tokenizer = Tokenizer::load(&dir).expect("应能打开词表");
    assert_eq!(tokenizer.special.audio_start, Some(151_669));
    assert_eq!(tokenizer.special.audio_pad, Some(151_676));
    assert_eq!(tokenizer.special.audio_end, Some(151_670));
    assert_eq!(tokenizer.special.eos, Some(151_643));
}

#[test]
fn encode_matches_reference_strings() {
    let Some(dir) = model_dir() else {
        eprintln!("跳过：没有 VOICE_MODEL_DIR");
        return;
    };
    let tokenizer = Tokenizer::load(&dir).expect("应能打开词表");
    // 参考值：mlx_qwen3_asr 对同一字符串的编码结果，见 dev-docs/notes/050。
    let cases: [(&str, &[u32]); 3] = [
        ("language Chinese", &[11_528, 8_453]),
        ("你好", &[108_386]),
        ("今天开会", &[100_644, 113_716]),
    ];
    for (text, expected) in cases {
        let actual = tokenizer.encode(text);
        assert_eq!(actual, expected, "{text} 的编码与参考不一致");
    }
}

#[test]
fn decode_round_trips_chinese() {
    let Some(dir) = model_dir() else {
        eprintln!("跳过：没有 VOICE_MODEL_DIR");
        return;
    };
    let tokenizer = Tokenizer::load(&dir).expect("应能打开词表");
    let ids = tokenizer.encode("今天开会");
    let text = tokenizer.decode(&ids);
    assert_eq!(text, "今天开会");
}

#[test]
fn special_token_decodes_to_its_literal() {
    let Some(dir) = model_dir() else {
        eprintln!("跳过：没有 VOICE_MODEL_DIR");
        return;
    };
    let tokenizer = Tokenizer::load(&dir).expect("应能打开词表");
    let text = tokenizer.decode(&[151_669, 151_676, 151_670]);
    assert_eq!(text, "<|audio_start|><|audio_pad|><|audio_end|>");
}

/// 产品路径的还原口径：去掉特殊 token 的字面量。
///
/// 参考 `manifest.greedy.text_from_tokens` 是 `"嗯。<|im_end|>"`；同一串 id
/// 去掉标记之后是 `"嗯。"`，与参考 `parse_asr_output` 交出的正文相同。
#[test]
fn decode_without_special_drops_the_markers() {
    let Some(dir) = model_dir() else {
        eprintln!("跳过：没有 VOICE_MODEL_DIR");
        return;
    };
    let tokenizer = Tokenizer::load(&dir).expect("应能打开词表");
    let ids = [106_287u32, 1_773, 151_645];
    assert_eq!(tokenizer.decode(&ids), "嗯。<|im_end|>");
    assert_eq!(tokenizer.decode_without_special(&ids), "嗯。");
    assert!(tokenizer.is_special_token(151_645));
    assert!(!tokenizer.is_special_token(106_287));
    // 只有标记时正文是空串，不回退成字面量。
    assert_eq!(tokenizer.decode_without_special(&[151_645]), "");
}
