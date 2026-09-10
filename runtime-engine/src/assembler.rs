//! 事件簿记：把 Adapter 事件聚合为 Block 与 Usage，支撑 partial()（总案 §25.1）。
//!
//! Adapter 发出的 Delta 是 append-only 增量；本模块按 block_id / call_id
//! 归属重组。畸形流（Delta 先于 Start、未知归属）静默忽略——
//! 由 Conformance fixtures 保证正常路径，Engine 只负责不 panic。

use runtime_core::content::ContentBlock;
use runtime_core::error::ModelError;
use runtime_core::event::ModelEvent;
use runtime_core::ids::{BlockId, CallId};
use runtime_core::invocation::PartialOutput;
use runtime_core::usage::Usage;

#[derive(Debug)]
enum AccBlock {
    Text {
        id: BlockId,
        buf: String,
    },
    Reasoning {
        id: BlockId,
        buf: String,
        payload: Option<serde_json::Value>,
    },
    ToolCall {
        call_id: CallId,
        name: String,
        args: String,
    },
}

#[derive(Debug, Default)]
pub struct StreamAssembler {
    blocks: Vec<AccBlock>,
    usage: Option<Usage>,
    seen_content: bool,
}

impl StreamAssembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// 吸收一个非终结事件。
    pub fn absorb(&mut self, event: &ModelEvent) {
        match event {
            ModelEvent::TextStarted { block_id } => {
                self.seen_content = true;
                self.blocks.push(AccBlock::Text {
                    id: block_id.clone(),
                    buf: String::new(),
                });
            }
            ModelEvent::TextDelta { block_id, delta } => {
                if let Some(AccBlock::Text { buf, .. }) = self
                    .blocks
                    .iter_mut()
                    .rev()
                    .find(|b| matches!(b, AccBlock::Text { id, .. } if *id == *block_id))
                {
                    buf.push_str(delta);
                }
            }
            ModelEvent::ReasoningStarted { block_id } => {
                self.seen_content = true;
                self.blocks.push(AccBlock::Reasoning {
                    id: block_id.clone(),
                    buf: String::new(),
                    payload: None,
                });
            }
            ModelEvent::ReasoningDelta { block_id, delta } => {
                if let Some(AccBlock::Reasoning { buf, .. }) = self
                    .blocks
                    .iter_mut()
                    .rev()
                    .find(|b| matches!(b, AccBlock::Reasoning { id, .. } if *id == *block_id))
                {
                    buf.push_str(delta);
                }
            }
            ModelEvent::ToolCallStarted { call_id, name } => {
                self.seen_content = true;
                self.blocks.push(AccBlock::ToolCall {
                    call_id: call_id.clone(),
                    name: name.clone(),
                    args: String::new(),
                });
            }
            ModelEvent::ToolCallDelta {
                call_id,
                arguments_json_delta,
            } => {
                let target = call_id.clone();
                if let Some(AccBlock::ToolCall { args, .. }) =
                    self.blocks.iter_mut().rev().find(
                        |b| matches!(b, AccBlock::ToolCall { call_id, .. } if *call_id == target),
                    )
                {
                    args.push_str(arguments_json_delta);
                }
            }
            ModelEvent::UsageUpdated { usage } => self.usage = Some(usage.clone()),
            // Ended / Finished / Started 等簿记事件不改变聚合状态
            _ => {}
        }
    }

    /// 是否已产出任何内容（Retry 安全重放判断的输入，§29）。
    pub fn seen_content(&self) -> bool {
        self.seen_content
    }

    pub fn usage(&self) -> Option<&Usage> {
        self.usage.as_ref()
    }

    /// 聚合后的文本内容（测试与诊断用）。
    pub fn text(&self) -> String {
        let mut out = String::new();
        for b in &self.blocks {
            if let AccBlock::Text { buf, .. } = b {
                out.push_str(buf);
            }
        }
        out
    }

    /// 给 Reasoning 块挂接 opaque 透传载荷（总案 §19.1）。
    ///
    /// Anthropic 的 thinking signature、OpenAI Responses 的
    /// encrypted_content 由此进入 Canonical 内容块，多轮回放时原样回传。
    /// 未签名块调用无效果（签名是可选的）。
    pub fn attach_reasoning_payload(&mut self, block_id: &BlockId, payload: serde_json::Value) {
        if let Some(AccBlock::Reasoning { payload: slot, .. }) = self
            .blocks
            .iter_mut()
            .rev()
            .find(|b| matches!(b, AccBlock::Reasoning { id, .. } if *id == *block_id))
        {
            *slot = Some(payload);
        }
    }

    /// 生成部分结果。失败 / 取消后必须可取回（总案 §25.1）。
    pub fn to_partial(&self, terminal_error: Option<ModelError>) -> PartialOutput {
        let content = self
            .blocks
            .iter()
            .map(|b| match b {
                AccBlock::Text { buf, .. } => ContentBlock::text(buf.clone()),
                AccBlock::Reasoning { buf, payload, .. } => {
                    ContentBlock::Reasoning(runtime_core::content::ReasoningBlock {
                        text: buf.clone(),
                        provider_payload: payload.clone(),
                    })
                }
                AccBlock::ToolCall {
                    call_id,
                    name,
                    args,
                } => ContentBlock::ToolCall(runtime_core::content::ToolCallBlock {
                    call_id: call_id.clone(),
                    name: name.clone(),
                    arguments_json: args.clone(),
                }),
            })
            .collect();
        PartialOutput {
            content,
            usage: self.usage.clone(),
            terminal_error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime_core::ids::BlockId;

    fn text_events() -> Vec<ModelEvent> {
        let id = BlockId::from("t1");
        vec![
            ModelEvent::TextStarted {
                block_id: id.clone(),
            },
            ModelEvent::TextDelta {
                block_id: id.clone(),
                delta: "你好".into(),
            },
            ModelEvent::TextDelta {
                block_id: id.clone(),
                delta: "，世界".into(),
            },
            ModelEvent::TextEnded { block_id: id },
        ]
    }

    #[test]
    fn deltas_are_appended_in_order() {
        let mut a = StreamAssembler::new();
        for e in text_events() {
            a.absorb(&e);
        }
        assert_eq!(a.text(), "你好，世界");
    }

    #[test]
    fn delta_without_start_is_ignored() {
        let mut a = StreamAssembler::new();
        a.absorb(&ModelEvent::TextDelta {
            block_id: BlockId::from("ghost"),
            delta: "x".into(),
        });
        assert_eq!(a.text(), "");
        assert!(!a.seen_content());
    }

    #[test]
    fn to_partial_carries_content_and_error() {
        let mut a = StreamAssembler::new();
        for e in text_events() {
            a.absorb(&e);
        }
        let p = a.to_partial(Some(ModelError::Cancelled));
        assert_eq!(p.content.len(), 1);
        assert_eq!(p.terminal_error, Some(ModelError::Cancelled));
    }

    #[test]
    fn reasoning_payload_attaches_and_flows_to_partial() {
        let rid = BlockId::from("r1");
        let mut a = StreamAssembler::new();
        a.absorb(&ModelEvent::ReasoningStarted {
            block_id: rid.clone(),
        });
        a.absorb(&ModelEvent::ReasoningDelta {
            block_id: rid.clone(),
            delta: "思考".into(),
        });
        a.attach_reasoning_payload(&rid, serde_json::json!({"signature": "sig-1"}));
        let p = a.to_partial(None);
        match &p.content[0] {
            ContentBlock::Reasoning(r) => {
                assert_eq!(r.text, "思考");
                assert_eq!(r.provider_payload.as_ref().unwrap()["signature"], "sig-1");
            }
            other => panic!("expected reasoning block, got {other:?}"),
        }
    }
}
