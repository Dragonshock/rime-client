//! 极小的 JSON 读取器，只够读 `manifest.json`。
//!
//! 对照箱子不引入序列化依赖：`manifest.json` 是本仓库自己导出的文件，
//! 结构固定，一个 200 行的递归下降解析器足够，且不会把第三方 crate 拖进离线构建。

/// 一个 JSON 值。数字统一按 `f64` 读，取整时由调用方自己决定。
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(fields) => fields
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Number(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_usize(&self) -> Option<usize> {
        let value = self.as_f64()?;
        if value < 0.0 || value.fract() != 0.0 {
            return None;
        }
        Some(value as usize)
    }

    pub fn as_u32(&self) -> Option<u32> {
        self.as_usize().map(|value| value as u32)
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&[(String, Json)]> {
        match self {
            Json::Object(fields) => Some(fields),
            _ => None,
        }
    }
}

/// 解析整份文本。错误里带字节偏移，方便对照导出脚本改字段。
pub fn parse(text: &str) -> Result<Json, String> {
    let bytes = text.as_bytes();
    let mut cursor = 0usize;
    let value = parse_value(bytes, &mut cursor)?;
    skip_ws(bytes, &mut cursor);
    if cursor != bytes.len() {
        return Err(format!("第 {cursor} 字节之后还有多余内容"));
    }
    Ok(value)
}

fn skip_ws(bytes: &[u8], cursor: &mut usize) {
    while *cursor < bytes.len() && matches!(bytes[*cursor], b' ' | b'\t' | b'\n' | b'\r') {
        *cursor += 1;
    }
}

fn parse_value(bytes: &[u8], cursor: &mut usize) -> Result<Json, String> {
    skip_ws(bytes, cursor);
    let byte = *bytes
        .get(*cursor)
        .ok_or_else(|| "文本提前结束".to_string())?;
    match byte {
        b'{' => parse_object(bytes, cursor),
        b'[' => parse_array(bytes, cursor),
        b'"' => parse_string(bytes, cursor).map(Json::Str),
        b't' => expect_keyword(bytes, cursor, "true").map(|_| Json::Bool(true)),
        b'f' => expect_keyword(bytes, cursor, "false").map(|_| Json::Bool(false)),
        b'n' => expect_keyword(bytes, cursor, "null").map(|_| Json::Null),
        b'-' | b'0'..=b'9' => parse_number(bytes, cursor),
        other => Err(format!("第 {} 字节出现意外的字符 {}", cursor, other as char)),
    }
}

fn expect_keyword(bytes: &[u8], cursor: &mut usize, word: &str) -> Result<(), String> {
    if bytes[*cursor..].starts_with(word.as_bytes()) {
        *cursor += word.len();
        Ok(())
    } else {
        Err(format!("第 {} 字节应为 {word}", cursor))
    }
}

fn parse_object(bytes: &[u8], cursor: &mut usize) -> Result<Json, String> {
    *cursor += 1; // '{'
    let mut fields = Vec::new();
    skip_ws(bytes, cursor);
    if bytes.get(*cursor) == Some(&b'}') {
        *cursor += 1;
        return Ok(Json::Object(fields));
    }
    loop {
        skip_ws(bytes, cursor);
        let key = parse_string(bytes, cursor)?;
        skip_ws(bytes, cursor);
        if bytes.get(*cursor) != Some(&b':') {
            return Err(format!("第 {} 字节缺少冒号", cursor));
        }
        *cursor += 1;
        let value = parse_value(bytes, cursor)?;
        fields.push((key, value));
        skip_ws(bytes, cursor);
        match bytes.get(*cursor) {
            Some(b',') => *cursor += 1,
            Some(b'}') => {
                *cursor += 1;
                return Ok(Json::Object(fields));
            }
            _ => return Err(format!("第 {} 字节对象没有闭合", cursor)),
        }
    }
}

fn parse_array(bytes: &[u8], cursor: &mut usize) -> Result<Json, String> {
    *cursor += 1; // '['
    let mut items = Vec::new();
    skip_ws(bytes, cursor);
    if bytes.get(*cursor) == Some(&b']') {
        *cursor += 1;
        return Ok(Json::Array(items));
    }
    loop {
        items.push(parse_value(bytes, cursor)?);
        skip_ws(bytes, cursor);
        match bytes.get(*cursor) {
            Some(b',') => *cursor += 1,
            Some(b']') => {
                *cursor += 1;
                return Ok(Json::Array(items));
            }
            _ => return Err(format!("第 {} 字节数组没有闭合", cursor)),
        }
    }
}

fn parse_string(bytes: &[u8], cursor: &mut usize) -> Result<String, String> {
    if bytes.get(*cursor) != Some(&b'"') {
        return Err(format!("第 {} 字节应为字符串", cursor));
    }
    *cursor += 1;
    let mut out = String::new();
    loop {
        let byte = *bytes
            .get(*cursor)
            .ok_or_else(|| "字符串提前结束".to_string())?;
        *cursor += 1;
        match byte {
            b'"' => return Ok(out),
            b'\\' => {
                let escape = *bytes
                    .get(*cursor)
                    .ok_or_else(|| "转义符提前结束".to_string())?;
                *cursor += 1;
                match escape {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{8}'),
                    b'f' => out.push('\u{c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let hex = bytes
                            .get(*cursor..*cursor + 4)
                            .ok_or_else(|| "\\u 转义不完整".to_string())?;
                        let text = std::str::from_utf8(hex)
                            .map_err(|_| "\\u 转义不是 ASCII".to_string())?;
                        let code = u32::from_str_radix(text, 16)
                            .map_err(|_| format!("\\u{text} 不是十六进制"))?;
                        out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        *cursor += 4;
                    }
                    other => return Err(format!("未知转义 \\{}", other as char)),
                }
            }
            _ => {
                // 多字节 UTF-8 原样拷回：manifest 里有中文与全角标点。
                let start = *cursor - 1;
                let width = utf8_width(byte);
                let end = start + width;
                let slice = bytes
                    .get(start..end)
                    .ok_or_else(|| "UTF-8 序列不完整".to_string())?;
                out.push_str(std::str::from_utf8(slice).map_err(|_| "非法 UTF-8".to_string())?);
                *cursor = end;
            }
        }
    }
}

fn utf8_width(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if first >> 5 == 0b110 {
        2
    } else if first >> 4 == 0b1110 {
        3
    } else {
        4
    }
}

fn parse_number(bytes: &[u8], cursor: &mut usize) -> Result<Json, String> {
    let start = *cursor;
    if bytes[*cursor] == b'-' {
        *cursor += 1;
    }
    while *cursor < bytes.len() {
        match bytes[*cursor] {
            b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-' => *cursor += 1,
            _ => break,
        }
    }
    let text = std::str::from_utf8(&bytes[start..*cursor])
        .map_err(|_| "数字不是 ASCII".to_string())?;
    text.parse::<f64>()
        .map(Json::Number)
        .map_err(|_| format!("第 {start} 字节的数字 {text} 无法解析"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_manifest_like_text() {
        let text = r#"{"a": [1, 2.5, -3], "b": {"c": "中文\u0021", "d": null}, "e": true}"#;
        let value = parse(text).expect("应能解析");
        assert_eq!(value.get("a").and_then(|v| v.as_array()).map(|v| v.len()), Some(3));
        assert_eq!(
            value
                .get("b")
                .and_then(|v| v.get("c"))
                .and_then(|v| v.as_str()),
            Some("中文!")
        );
        assert_eq!(value.get("e"), Some(&Json::Bool(true)));
    }

    #[test]
    fn rejects_truncated_text() {
        assert!(parse(r#"{"a": 1"#).is_err());
    }
}
