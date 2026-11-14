//! 热词放在系统前缀，语言标签放在助手前缀。
//! 热词缺席时不写系统前缀，也不把空串写进去。

/// 前缀里的一段。每一段写进模型的文本都不是空串。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrefixPart {
    /// 系统前缀中的热词文本，调用方保证非空。
    System(String),
    /// 用户轮次。音频占位由编码器 token 数决定，不在这里塞空串。
    User,
    /// 助手前缀中的语言标签，例如 `Chinese`。
    AssistantLanguage(String),
}

impl PrefixPart {
    /// 这一段实际写进前缀的文本。
    pub fn written(&self) -> &str {
        match self {
            PrefixPart::System(text) => text,
            PrefixPart::User => "<|im_start|>user",
            PrefixPart::AssistantLanguage(language) => language,
        }
    }
}

/// 组装前缀。`hotwords` 为 `None` 或空串时，结果里没有系统段。
pub fn build_prefix(hotwords: Option<&str>, language: &str) -> Vec<PrefixPart> {
    let mut parts = Vec::new();
    if let Some(text) = hotwords {
        if !text.is_empty() {
            parts.push(PrefixPart::System(text.to_string()));
        }
    }
    parts.push(PrefixPart::User);
    if !language.is_empty() {
        parts.push(PrefixPart::AssistantLanguage(language.to_string()));
    }
    parts
}

/// 拼成送进解码器的前缀字符串。热词缺席时不会出现系统段。
pub fn render_prefix(parts: &[PrefixPart]) -> String {
    let mut out = String::new();
    for part in parts {
        match part {
            PrefixPart::System(text) => {
                out.push_str("<|im_start|>system\n");
                out.push_str(text);
                out.push_str("<|im_end|>\n");
            }
            PrefixPart::User => out.push_str("<|im_start|>user\n"),
            PrefixPart::AssistantLanguage(language) => {
                out.push_str("<|im_start|>assistant\nlanguage ");
                out.push_str(language);
            }
        }
    }
    out
}

/// 音频占位：`audio_start`、`audio_pad` 重复 `n` 次、`audio_end`。
pub fn audio_placeholder_ids(n: usize, start: u32, pad: u32, end: u32) -> Vec<u32> {
    let mut ids = Vec::with_capacity(n + 2);
    ids.push(start);
    ids.extend(std::iter::repeat(pad).take(n));
    ids.push(end);
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_hotwords_do_not_write_empty_string() {
        for hotwords in [None, Some("")] {
            let parts = build_prefix(hotwords, "Chinese");
            assert!(
                parts.iter().all(|part| !part.written().is_empty()),
                "空串不得写入前缀: {hotwords:?}"
            );
            assert!(
                !parts
                    .iter()
                    .any(|part| matches!(part, PrefixPart::System(_))),
                "热词缺席时不写系统前缀"
            );
            let rendered = render_prefix(&parts);
            assert!(!rendered.contains("<|im_start|>system"));
            assert!(rendered.contains("language Chinese"));
            assert!(
                !rendered.contains("language \n") && !parts.iter().any(|p| p.written().is_empty())
            );
        }
    }

    #[test]
    fn present_hotwords_go_into_system_prefix() {
        let parts = build_prefix(Some("专名"), "Chinese");
        assert!(parts
            .iter()
            .any(|part| matches!(part, PrefixPart::System(text) if text == "专名")));
        assert!(parts.iter().all(|part| !part.written().is_empty()));
        let rendered = render_prefix(&parts);
        assert!(rendered.contains("<|im_start|>system\n专名<|im_end|>\n"));
        assert!(rendered.contains("language Chinese"));
    }
}
