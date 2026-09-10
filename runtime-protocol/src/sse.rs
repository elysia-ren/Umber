//! SSE 行装配（Server-Sent Events）。
//!
//! 按 SSE 规范以空行分派事件：`event:` 行给事件命名（Anthropic / Gemini
//! 需要），`data:` 行累计载荷；OpenAI 系只有 `data:`，按数据分派。
//! 注释行（`:` 开头）与 `retry:` 忽略。

/// 一条完整的 SSE 事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub name: Option<String>,
    pub data: String,
}

#[derive(Debug, Default)]
pub struct SseAssembler {
    event_name: Option<String>,
    data_lines: Vec<String>,
}

impl SseAssembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// 喂入一行（不含行尾）。空行返回装配完成的事件，无事件则 `None`。
    pub fn feed_line(&mut self, line: &str) -> Option<SseEvent> {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            if self.data_lines.is_empty() && self.event_name.is_none() {
                return None; // 连续空行
            }
            let event = SseEvent {
                name: self.event_name.take(),
                data: self.data_lines.join("\n"),
            };
            self.data_lines.clear();
            return Some(event);
        }
        if let Some(rest) = line.strip_prefix(':') {
            let _ = rest; // 注释 / 心跳
            return None;
        }
        if let Some(name) = line.strip_prefix("event:") {
            self.event_name = Some(name.trim().to_string());
            return None;
        }
        if let Some(data) = line.strip_prefix("data:") {
            self.data_lines
                .push(data.strip_prefix(' ').unwrap_or(data).to_string());
            return None;
        }
        // retry: 等其他字段忽略
        None
    }
}

/// `data: [DONE]` 判定（OpenAI 系流结束标记）。
pub fn is_done_marker(event: &SseEvent) -> bool {
    event.name.is_none() && event.data.trim() == "[DONE]"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_style_data_only_events() {
        let mut sse = SseAssembler::new();
        let input = "data: {\"a\":1}\n\ndata: [DONE]\n\n";
        let mut events = Vec::new();
        for line in input.lines() {
            if let Some(e) = sse.feed_line(line) {
                events.push(e);
            }
        }
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].data, "{\"a\":1}");
        assert!(is_done_marker(&events[1]));
    }

    #[test]
    fn anthropic_style_named_events() {
        let mut sse = SseAssembler::new();
        let input = "event: content_block_delta\ndata: {\"i\":1}\n\nevent: ping\ndata: {}\n\n";
        let mut events = Vec::new();
        for line in input.lines() {
            if let Some(e) = sse.feed_line(line) {
                events.push(e);
            }
        }
        assert_eq!(events[0].name.as_deref(), Some("content_block_delta"));
        assert_eq!(events[0].data, "{\"i\":1}");
        assert_eq!(events[1].name.as_deref(), Some("ping"));
    }

    #[test]
    fn comments_and_crlf_are_tolerated() {
        let mut sse = SseAssembler::new();
        assert!(sse.feed_line(": keep-alive").is_none());
        assert!(sse.feed_line("data: {\"x\":1}\r").is_none());
        let e = sse.feed_line("").unwrap();
        assert_eq!(e.data, "{\"x\":1}");
    }
}
