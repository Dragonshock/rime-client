//! Qwen3-ASR 的字节级 BPE。
//!
//! 词表在模型目录的 `vocab.json`（151643 条基础词），特殊 token 在
//! `tokenizer_config.json` 的 `added_tokens_decoder` 里。合并在 `merges.txt`。
//!
//! 只做两件事：把特殊 token 与普通文本编码成 id，以及把 id 序列还原成文本。
//! 编码用的预切分与 GPT-2 一致：把连续字母（含 CJK）、连续数字、连续其它符号
//! 各自成块，前导空格跟着后面的块。这里不实现正则引擎，按字符类别走一遍。

use std::collections::HashMap;
use std::path::Path;

/// 特殊 token。id 由 `tokenizer_config.json` 给出，不写死在这里。
#[derive(Clone, Debug, Default)]
pub struct SpecialTokens {
    pub audio_start: Option<u32>,
    pub audio_pad: Option<u32>,
    pub audio_end: Option<u32>,
    pub im_start: Option<u32>,
    pub im_end: Option<u32>,
    pub asr_text: Option<u32>,
    pub eos: Option<u32>,
}

pub struct Tokenizer {
    /// id → 字节串。
    pieces: Vec<Vec<u8>>,
    /// 字节串 → id。
    ids: HashMap<Vec<u8>, u32>,
    /// 合并优先级：`(左, 右)` → 名次。名次小的先合并。
    ranks: HashMap<(Vec<u8>, Vec<u8>), u32>,
    pub special: SpecialTokens,
}

impl Tokenizer {
    pub fn load(model_dir: &Path) -> Result<Self, String> {
        let vocab_text = std::fs::read_to_string(model_dir.join("vocab.json"))
            .map_err(|_| "打不开 vocab.json".to_string())?;
        let entries = parse_flat_object(&vocab_text)?;
        let mut pieces: Vec<Vec<u8>> = Vec::new();
        let mut ids: HashMap<Vec<u8>, u32> = HashMap::new();
        for (piece, id) in entries {
            let index = id as usize;
            if pieces.len() <= index {
                pieces.resize(index + 1, Vec::new());
            }
            let bytes = piece.into_bytes();
            pieces[index] = bytes.clone();
            ids.insert(bytes, id);
        }

        let mut special = SpecialTokens::default();
        if let Ok(text) = std::fs::read_to_string(model_dir.join("tokenizer_config.json")) {
            special = parse_special_tokens(&text);
            // 特殊 token 也进词表，才能把 id 还原成 `<|audio_pad|>` 这类字面量。
            for (name, id) in [
                ("<|audio_start|>", special.audio_start),
                ("<|audio_pad|>", special.audio_pad),
                ("<|audio_end|>", special.audio_end),
                ("<|im_start|>", special.im_start),
                ("<|im_end|>", special.im_end),
                ("<asr_text>", special.asr_text),
            ] {
                if let Some(id) = id {
                    let index = id as usize;
                    if pieces.len() <= index {
                        pieces.resize(index + 1, Vec::new());
                    }
                    let bytes = name.as_bytes().to_vec();
                    pieces[index] = bytes.clone();
                    ids.insert(bytes, id);
                }
            }
        }

        let mut ranks = HashMap::new();
        if let Ok(text) = std::fs::read_to_string(model_dir.join("merges.txt")) {
            for (rank, line) in text.lines().enumerate() {
                if line.starts_with("#version") || line.trim().is_empty() {
                    continue;
                }
                let mut parts = line.split(' ');
                let (Some(left), Some(right)) = (parts.next(), parts.next()) else {
                    continue;
                };
                // 同一个 pair 若重复出现，先出现的那行名次更靠前，保留它。
                ranks
                    .entry((left.as_bytes().to_vec(), right.as_bytes().to_vec()))
                    .or_insert(rank as u32);
            }
        }

        Ok(Self {
            pieces,
            ids,
            ranks,
            special,
        })
    }

    pub fn id_of(&self, piece: &str) -> Option<u32> {
        self.ids.get(piece.as_bytes()).copied()
    }

    /// 文本 → id。特殊 token 单独识别；其余按 GPT-2 预切分后做 BPE。
    pub fn encode(&self, text: &str) -> Vec<u32> {
        let mut out = Vec::new();
        for chunk in split_special(text) {
            match chunk {
                Chunk::Special(name) => {
                    if let Some(id) = self.id_of(&name) {
                        out.push(id);
                    }
                }
                Chunk::Text(text) => {
                    for word in pretokenize(&text) {
                        out.extend(self.bpe(&word));
                    }
                }
            }
        }
        out
    }

    /// id → 字节。特殊 token 原样给出字面量。
    pub fn decode(&self, ids: &[u32]) -> String {
        let mut bytes = Vec::new();
        for id in ids {
            if let Some(piece) = self.pieces.get(*id as usize) {
                if is_special_piece(piece) {
                    bytes.extend_from_slice(piece);
                } else {
                    bytes.extend_from_slice(&decode_bytes(piece));
                }
            }
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// 这个 id 的字符串是不是特殊 token 的字面量（`<|…|>` 或 `<asr_text>`）。
    pub fn is_special_token(&self, id: u32) -> bool {
        self.pieces
            .get(id as usize)
            .map(|piece| is_special_piece(piece))
            .unwrap_or(false)
    }

    /// id → 文本，去掉特殊 token 的字面量。产品路径用它：提交给输入法的
    /// 一串字里不该出现 `<|im_end|>`、`<asr_text>` 这类标记。
    pub fn decode_without_special(&self, ids: &[u32]) -> String {
        let kept: Vec<u32> = ids
            .iter()
            .copied()
            .filter(|id| !self.is_special_token(*id))
            .collect();
        self.decode(&kept)
    }

    fn bpe(&self, word: &[u8]) -> Vec<u32> {
        // 每个原始字节先映射成一个可见字符，再按字节串做合并。
        let mut symbols: Vec<Vec<u8>> = word
            .iter()
            .map(|byte| encoded_char(*byte).to_string().into_bytes())
            .collect();
        while symbols.len() > 1 {
            let mut best: Option<(u32, usize)> = None;
            for index in 0..symbols.len() - 1 {
                let key = (symbols[index].clone(), symbols[index + 1].clone());
                if let Some(rank) = self.ranks.get(&key) {
                    if best.map_or(true, |(current, _)| *rank < current) {
                        best = Some((*rank, index));
                    }
                }
            }
            let Some((_, at)) = best else { break };
            let right = symbols.remove(at + 1);
            symbols[at].extend_from_slice(&right);
        }
        symbols
            .into_iter()
            .filter_map(|symbol| self.ids.get(&symbol).copied())
            .collect()
    }
}

enum Chunk {
    Special(String),
    Text(String),
}

fn split_special(text: &str) -> Vec<Chunk> {
    const NAMES: [&str; 6] = [
        "<|audio_start|>",
        "<|audio_pad|>",
        "<|audio_end|>",
        "<|im_start|>",
        "<|im_end|>",
        "<asr_text>",
    ];
    let mut out = Vec::new();
    let mut rest = text;
    loop {
        let mut earliest: Option<(usize, &str)> = None;
        for name in NAMES {
            if let Some(at) = rest.find(name) {
                if earliest.map_or(true, |(current, _)| at < current) {
                    earliest = Some((at, name));
                }
            }
        }
        let Some((at, name)) = earliest else {
            if !rest.is_empty() {
                out.push(Chunk::Text(rest.to_string()));
            }
            break;
        };
        if at > 0 {
            out.push(Chunk::Text(rest[..at].to_string()));
        }
        out.push(Chunk::Special(name.to_string()));
        rest = &rest[at + name.len()..];
    }
    out
}

fn pretokenize(text: &str) -> Vec<Vec<u8>> {
    // 与 GPT-2 的切分一致：一个单词可以带一个前导空格，之后同类字符连续成块。
    let mut words: Vec<Vec<u8>> = Vec::new();
    let mut current = String::new();
    // 上一块所属的字符类别；4 表示「当前只有前导空格」，等待后续字符决定类别。
    let mut last_class = 0u8;
    for ch in text.chars() {
        let class = classify(ch);
        if class == CLASS_SPACE {
            if current.is_empty() {
                current.push(ch);
                last_class = CLASS_PENDING;
            } else {
                words.push(current.clone().into_bytes());
                current.clear();
                current.push(ch);
                last_class = CLASS_PENDING;
            }
            continue;
        }
        if last_class == CLASS_PENDING {
            current.push(ch);
            last_class = class;
            continue;
        }
        if class == last_class {
            current.push(ch);
            continue;
        }
        if !current.is_empty() {
            words.push(current.clone().into_bytes());
            current.clear();
        }
        current.push(ch);
        last_class = class;
    }
    if !current.is_empty() {
        words.push(current.into_bytes());
    }
    words
}

/// 表意文字与假名、谚文。全角标点不算字母，参考实现的预切分把它们当符号。
fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3040..=0x30FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2FA1F
    )
}

const CLASS_SPACE: u8 = 0;
const CLASS_LETTER: u8 = 1;
const CLASS_DIGIT: u8 = 2;
const CLASS_OTHER: u8 = 3;
const CLASS_PENDING: u8 = 4;

/// 0 空白、1 字母（含 CJK）、2 数字、3 其它。
fn classify(ch: char) -> u8 {
    if ch.is_whitespace() {
        CLASS_SPACE
    } else if ch.is_numeric() {
        CLASS_DIGIT
    } else if ch.is_alphabetic() || is_cjk(ch) {
        CLASS_LETTER
    } else {
        CLASS_OTHER
    }
}

fn is_special_piece(piece: &[u8]) -> bool {
    piece.starts_with(b"<|") || piece.starts_with(b"<asr_text>")
}

/// GPT-2 的字节到可见字符映射。
fn decode_bytes(piece: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(piece.len());
    for ch in String::from_utf8_lossy(piece).chars() {
        if let Some(byte) = visible_to_byte(ch) {
            out.push(byte);
        } else {
            let mut buffer = [0u8; 4];
            out.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
        }
    }
    out
}

fn encoded_char(byte: u8) -> char {
    BYTE_TABLE[byte as usize]
}

fn visible_to_byte(ch: char) -> Option<u8> {
    BYTE_TABLE
        .iter()
        .position(|candidate| *candidate == ch)
        .map(|index| index as u8)
}

static BYTE_TABLE: std::sync::LazyLock<[char; 256]> = std::sync::LazyLock::new(byte_table);

/// 256 个字节对应的可见字符，顺序与 GPT-2 一致。
fn byte_table() -> [char; 256] {
    // 用 '\0' 当占位符：空格字节 0x20 自己也要映射成 'Ġ'（U+0120），不能拿 ' ' 当哨兵。
    let mut table = ['\0'; 256];
    for byte in 0u32..256 {
        let visible = (33..=126).contains(&byte) || (161..=172).contains(&byte) || byte >= 174;
        if visible {
            table[byte as usize] = char::from_u32(byte).unwrap_or('\u{fffd}');
        }
    }
    let mut extra = 0u32;
    for byte in 0u32..256 {
        if table[byte as usize] != '\0' {
            continue;
        }
        table[byte as usize] = char::from_u32(256 + extra).unwrap_or('\u{fffd}');
        extra += 1;
    }
    table
}

/// 解析 `{"词": id, ...}` 这种一层对象。只处理字符串键、整数值和 JSON 转义。
fn parse_flat_object(text: &str) -> Result<Vec<(String, u32)>, String> {
    let mut out = Vec::new();
    let bytes: Vec<char> = text.chars().collect();
    let mut index = 0usize;
    let len = bytes.len();
    let skip_ws = |index: &mut usize| {
        while *index < len && bytes[*index].is_whitespace() {
            *index += 1;
        }
    };
    while index < len && bytes[index] != '{' {
        index += 1;
    }
    index += 1;
    loop {
        skip_ws(&mut index);
        if index >= len || bytes[index] == '}' {
            break;
        }
        if bytes[index] != '"' {
            return Err("词表键不是字符串".into());
        }
        index += 1;
        let mut key = String::new();
        while index < len {
            let ch = bytes[index];
            if ch == '\\' {
                index += 1;
                if index >= len {
                    return Err("词表转义不完整".into());
                }
                let escaped = bytes[index];
                match escaped {
                    'u' => {
                        let hex: String = bytes
                            .get(index + 1..index + 5)
                            .ok_or("词表 \\u 越界")?
                            .iter()
                            .collect();
                        let code = u32::from_str_radix(&hex, 16).map_err(|_| "词表 \\u 无效")?;
                        key.push(char::from_u32(code).ok_or("词表码点无效")?);
                        index += 5;
                        continue;
                    }
                    'n' => key.push('\n'),
                    't' => key.push('\t'),
                    'r' => key.push('\r'),
                    'b' => key.push('\u{8}'),
                    'f' => key.push('\u{c}'),
                    '"' => key.push('"'),
                    '\\' => key.push('\\'),
                    '/' => key.push('/'),
                    other => key.push(other),
                }
                index += 1;
                continue;
            }
            if ch == '"' {
                index += 1;
                break;
            }
            key.push(ch);
            index += 1;
        }
        skip_ws(&mut index);
        if index >= len || bytes[index] != ':' {
            return Err("词表缺少冒号".into());
        }
        index += 1;
        skip_ws(&mut index);
        let start = index;
        while index < len && (bytes[index].is_ascii_digit() || bytes[index] == '-') {
            index += 1;
        }
        let number: String = bytes[start..index].iter().collect();
        let id: u32 = number.parse().map_err(|_| "词表 id 不是整数")?;
        out.push((key, id));
        skip_ws(&mut index);
        if index < len && bytes[index] == ',' {
            index += 1;
        }
    }
    Ok(out)
}

/// 从 `added_tokens_decoder` 里取特殊 token 的 id。
fn parse_special_tokens(text: &str) -> SpecialTokens {
    let mut special = SpecialTokens::default();
    let pairs = [
        (
            "<|audio_start|>",
            &mut special.audio_start as *mut Option<u32>,
        ),
        ("<|audio_pad|>", &mut special.audio_pad as *mut Option<u32>),
        ("<|audio_end|>", &mut special.audio_end as *mut Option<u32>),
        ("<|im_start|>", &mut special.im_start as *mut Option<u32>),
        ("<|im_end|>", &mut special.im_end as *mut Option<u32>),
        ("<asr_text>", &mut special.asr_text as *mut Option<u32>),
        ("<|endoftext|>", &mut special.eos as *mut Option<u32>),
    ];
    for (name, slot) in pairs {
        if let Some(id) = find_added_token(text, name) {
            unsafe {
                *slot = Some(id);
            }
        }
    }
    special
}

/// 在 `"<id>": { ... "content": "<name>" ... }` 里找 id。
///
/// 从 `content` 出现的位置往前看，最近的、能解析成整数的引号串就是这条特殊 token 的 id。
fn find_added_token(text: &str, name: &str) -> Option<u32> {
    let needle = format!("\"{name}\"");
    let content_at = text.find(&needle)?;
    let before = &text[..content_at];
    let quotes: Vec<usize> = before
        .char_indices()
        .filter(|(_, ch)| *ch == '"')
        .map(|(index, _)| index)
        .collect();
    for pair in quotes.windows(2).rev() {
        let candidate = &before[pair[0] + 1..pair[1]];
        if let Ok(id) = candidate.parse::<u32>() {
            return Some(id);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_table_round_trips() {
        for byte in 0u16..256 {
            let ch = BYTE_TABLE[byte as usize];
            assert_eq!(
                visible_to_byte(ch),
                Some(byte as u8),
                "字节 {byte} 往返失败"
            );
        }
    }

    #[test]
    fn flat_object_parses_escapes() {
        let entries = parse_flat_object(r#"{"a":1,"b\"c":2,"\u00e4":3}"#).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0], ("a".to_string(), 1));
        assert_eq!(entries[1], ("b\"c".to_string(), 2));
        assert_eq!(entries[2], ("ä".to_string(), 3));
    }

    #[test]
    fn added_token_id_is_read() {
        let text = r#""added_tokens_decoder": { "151676": { "content": "<|audio_pad|>" } }"#;
        assert_eq!(find_added_token(text, "<|audio_pad|>"), Some(151676));
    }

    #[test]
    fn pretokenize_splits_letters_digits_and_marks() {
        let words = pretokenize("ab 12，cd");
        assert_eq!(words.len(), 4, "实际 {words:?}");
    }
}
