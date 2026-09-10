//! 事件流断言库（总案 §52）。
//!
//! 约定返回 `Result<(), String>`，由测试决定如何呈现失败。

use std::collections::BTreeMap;

use umber_core::event::{SequenceValidator, SequencedEvent};

/// sequence 必须从 0 开始且严格递增（总案 §24）。
pub fn check_monotonic(events: &[SequencedEvent]) -> Result<(), String> {
    let mut v = SequenceValidator::new();
    for (i, e) in events.iter().enumerate() {
        if i == 0 && e.sequence != 0 {
            return Err(format!("first sequence must be 0, got {}", e.sequence));
        }
        v.check(e.sequence)
            .map_err(|err| format!("at index {i}: {err}"))?;
    }
    Ok(())
}

/// 终结事件恰好一个（总案 §23.1）。
pub fn check_single_terminal(events: &[SequencedEvent]) -> Result<(), String> {
    let terminals: Vec<_> = events.iter().filter(|e| e.event.is_terminal()).collect();
    match terminals.len() {
        1 => Ok(()),
        n => Err(format!("expected exactly 1 terminal event, found {n}")),
    }
}

pub fn terminal(events: &[SequencedEvent]) -> Option<&SequencedEvent> {
    events.iter().find(|e| e.event.is_terminal())
}

/// 按 block 重组文本（append-only Delta）。
pub fn reassembled_text(events: &[SequencedEvent]) -> String {
    let mut blocks: Vec<(String, String)> = Vec::new();
    let mut index: BTreeMap<String, usize> = BTreeMap::new();
    for e in events {
        match &e.event {
            umber_core::ModelEvent::TextStarted { block_id } => {
                index.insert(block_id.to_string(), blocks.len());
                blocks.push((block_id.to_string(), String::new()));
            }
            umber_core::ModelEvent::TextDelta { block_id, delta } => {
                if let Some(&i) = index.get(block_id.as_ref()) {
                    blocks[i].1.push_str(delta);
                }
            }
            _ => {}
        }
    }
    blocks.into_iter().map(|(_, text)| text).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use umber_core::event::ModelEvent;
    use umber_core::ids::BlockId;

    fn text_stream() -> Vec<SequencedEvent> {
        let id = BlockId::from("b1");
        vec![
            SequencedEvent {
                sequence: 0,
                event: ModelEvent::TextStarted {
                    block_id: id.clone(),
                },
            },
            SequencedEvent {
                sequence: 1,
                event: ModelEvent::TextDelta {
                    block_id: id.clone(),
                    delta: "你好".into(),
                },
            },
            SequencedEvent {
                sequence: 2,
                event: ModelEvent::TextDelta {
                    block_id: id.clone(),
                    delta: "，世界".into(),
                },
            },
            SequencedEvent {
                sequence: 3,
                event: ModelEvent::Cancelled,
            },
        ]
    }

    #[test]
    fn checks_pass_on_well_formed_stream() {
        let events = text_stream();
        assert!(check_monotonic(&events).is_ok());
        assert!(check_single_terminal(&events).is_ok());
        assert_eq!(reassembled_text(&events), "你好，世界");
    }

    #[test]
    fn duplicate_sequence_fails() {
        let mut events = text_stream();
        events[2].sequence = 1;
        assert!(check_monotonic(&events).is_err());
    }

    #[test]
    fn two_terminals_fail() {
        let mut events = text_stream();
        events.push(SequencedEvent {
            sequence: 4,
            event: ModelEvent::Cancelled,
        });
        assert!(check_single_terminal(&events).is_err());
    }
}
