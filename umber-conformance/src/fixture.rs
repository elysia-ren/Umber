//! Fixture 文件格式（总案 §52）：每协议 golden request / response / stream 序列。

use serde::{Deserialize, Serialize};

use umber_core::event::SequencedEvent;
use umber_core::request::GenerateRequest;
use umber_core::response::StopReason;

/// 一个 fixture = 名称 + 请求 + 期望的事件流 + 期望结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fixture {
    pub name: String,
    pub request: GenerateRequest,
    pub events: Vec<SequencedEvent>,
    #[serde(default)]
    pub expect: Expectation,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Expectation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl Fixture {
    pub fn to_json_str(&self) -> String {
        serde_json::to_string_pretty(self).expect("fixture serialization cannot fail")
    }

    pub fn from_json_str(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use umber_core::event::ModelEvent;
    use umber_core::ids::BlockId;
    use umber_core::message::Message;

    #[test]
    fn fixture_roundtrips_through_json() {
        let fixture = Fixture {
            name: "plain_text".into(),
            request: GenerateRequest::new("dep-1", vec![Message::user("hi")]),
            events: vec![SequencedEvent {
                sequence: 0,
                event: ModelEvent::TextStarted {
                    block_id: BlockId::from("b"),
                },
            }],
            expect: Expectation {
                stop_reason: Some(StopReason::EndTurn),
                text: Some("hello".into()),
            },
        };
        let json = fixture.to_json_str();
        let back = Fixture::from_json_str(&json).unwrap();
        assert_eq!(back, fixture);
    }
}
