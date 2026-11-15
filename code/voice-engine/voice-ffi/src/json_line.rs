//! 一行 JSON 的回复。
//!
//! 键就是 Swift 侧 `RecognitionReplyParser` 读的那六个：`session`、`text`、
//! `decoded`、`kind`、`ok`、`message`。解释器那条路（`worker.py`）写同一组键，
//! 两条路的回复因此同形，解析器只有一处。
//!
//! `kind` 有三种：`final` / `partial` 是识别结果，`replaced` 是一条会话生命期通知
//! （同号的旧会话被替换掉了）。`replaced` 不带文本，Swift 侧只把它写进 `voice.log`，
//! 不当成一次回包派给门。
//!
//! 只做序列化。末尾不加换行；调用方负责复制，字符串只活到这次回调返回。

use std::fmt::Write;

/// 一条带文本的回复。`kind` 是 `final` 或 `partial`。
///
/// `decoded` 为真：这条回复带着一段解出来的文本（可能是空串）。部分结果只在文本
/// 变化时才发，所以真就表示「这一段是新的」。定稿不看这一位。
pub(crate) fn reply_line(session: u64, text: &str, kind: &str) -> String {
    compose(session, text, Some(kind), true, true, None)
}

/// 一条「同号旧会话已被替换」的通知。文本为空，`ok` 为真，`decoded` 为假。
///
/// 撞号时先看这一行：它说明引擎在这一侧把那枚旧会话丢掉、按新参数重建了。
pub(crate) fn replaced_line(session: u64) -> String {
    compose(
        session,
        "",
        Some("replaced"),
        true,
        false,
        Some("同号旧会话已丢弃"),
    )
}

/// 一条失败回复。文本为空，`kind` 不写，`ok` 为假，原因写进 `message`。
pub(crate) fn failure_line(session: u64, message: &str) -> String {
    compose(session, "", None, false, false, Some(message))
}

fn compose(
    session: u64,
    text: &str,
    kind: Option<&str>,
    ok: bool,
    decoded: bool,
    message: Option<&str>,
) -> String {
    let mut items: Vec<(&str, String)> = Vec::with_capacity(6);
    items.push(("decoded", decoded.to_string()));
    if let Some(kind) = kind {
        items.push(("kind", quote(kind)));
    }
    if let Some(message) = message {
        items.push(("message", quote(message)));
    }
    items.push(("ok", ok.to_string()));
    items.push(("session", session.to_string()));
    items.push(("text", quote(text)));
    let mut out = String::with_capacity(64 + text.len());
    out.push('{');
    for (index, (key, value)) in items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(key);
        out.push_str("\":");
        out.push_str(value);
    }
    out.push('}');
    out
}

fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            control if (control as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", control as u32);
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_text_line_carries_the_six_keys() {
        let line = reply_line(7, "你好", "final");
        assert_eq!(
            line,
            "{\"decoded\":true,\"kind\":\"final\",\"ok\":true,\"session\":7,\"text\":\"你好\"}"
        );
        let partial = reply_line(8, "你", "partial");
        assert_eq!(
            partial,
            "{\"decoded\":true,\"kind\":\"partial\",\"ok\":true,\"session\":8,\"text\":\"你\"}"
        );
    }

    #[test]
    fn failure_line_is_not_ok_and_carries_the_reason() {
        let line = failure_line(3, "只有一 token");
        assert_eq!(
            line,
            "{\"decoded\":false,\"message\":\"只有一 token\",\"ok\":false,\"session\":3,\"text\":\"\"}"
        );
    }

    #[test]
    fn quotes_quotes_backslashes_and_controls() {
        let line = reply_line(1, "a\"b\\c\nd\te\u{1}", "final");
        assert!(
            line.contains("\"text\":\"a\\\"b\\\\c\\nd\\te\\u0001\""),
            "{line}"
        );
    }

    #[test]
    fn replaced_line_carries_no_text_and_says_so_in_the_message() {
        let line = replaced_line(4);
        assert_eq!(
            line,
            "{\"decoded\":false,\"kind\":\"replaced\",\"message\":\"同号旧会话已丢弃\",\"ok\":true,\"session\":4,\"text\":\"\"}"
        );
    }

    #[test]
    fn keys_are_sorted_and_nothing_else_is_written() {
        let line = reply_line(9, "", "partial");
        assert_eq!(
            line,
            "{\"decoded\":true,\"kind\":\"partial\",\"ok\":true,\"session\":9,\"text\":\"\"}"
        );
        assert!(!line.ends_with('\n'));
    }
}
