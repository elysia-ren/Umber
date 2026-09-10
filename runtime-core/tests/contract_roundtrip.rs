//! M0 契约集成测试：一个完整调用场景在 JSON 序列化边界上无损往返。
//! 这是"Rust 类型为唯一真值、schema 生成一致"的基础。

use runtime_core::content::{ReasoningBlock, TextBlock, ToolCallBlock, ToolResultBlock};
use runtime_core::error::{ErrorDetail, ModelError};
use runtime_core::event::{ModelEvent, SequenceValidator, SequencedEvent};
use runtime_core::ids::{BlockId, CallId, DeploymentId, InvocationId};
use runtime_core::invocation::{Invocation, InvocationState, PartialOutput};
use runtime_core::message::Message;
use runtime_core::request::{GenerateRequest, Tool, ToolChoice};
use runtime_core::response::{GenerateResponse, StopReason};
use runtime_core::usage::Usage;

#[test]
fn full_invocation_scenario_roundtrips_through_json() {
    // 1. 请求
    let mut request = GenerateRequest::new(
        DeploymentId::from("deepseek/official/openai_chat/deepseek-chat"),
        vec![
            Message::system("你是一个助手"),
            Message::user("查一下天气"),
            Message::assistant(vec![runtime_core::ContentBlock::ToolCall(ToolCallBlock {
                call_id: CallId::from("call-1"),
                name: "get_weather".into(),
                arguments_json: r#"{"city":"杭州"}"#.into(),
            })]),
            Message::new(
                runtime_core::Role::Tool,
                vec![runtime_core::ContentBlock::ToolResult(ToolResultBlock {
                    call_id: CallId::from("call-1"),
                    is_error: false,
                    content: vec![runtime_core::ContentBlock::text("晴 28°C")],
                })],
            ),
        ],
    );
    request.tools = vec![Tool {
        name: "get_weather".into(),
        description: "查询城市天气".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "required": ["city"]
        }),
    }];
    request.tool_choice = ToolChoice::Auto;
    request.validate().expect("request must be valid");

    // 2. 事件流（全局单调 sequence）
    let events = vec![
        SequencedEvent {
            sequence: 0,
            event: ModelEvent::Started {
                invocation_id: InvocationId::from("inv-1"),
            },
        },
        SequencedEvent {
            sequence: 1,
            event: ModelEvent::ReasoningStarted {
                block_id: BlockId::from("r1"),
            },
        },
        SequencedEvent {
            sequence: 2,
            event: ModelEvent::ReasoningDelta {
                block_id: BlockId::from("r1"),
                delta: "用户想查天气".into(),
            },
        },
        SequencedEvent {
            sequence: 3,
            event: ModelEvent::ReasoningEnded {
                block_id: BlockId::from("r1"),
            },
        },
        SequencedEvent {
            sequence: 4,
            event: ModelEvent::ToolCallStarted {
                call_id: CallId::from("call-2"),
                name: "get_weather".into(),
            },
        },
        SequencedEvent {
            sequence: 5,
            event: ModelEvent::ToolCallDelta {
                call_id: CallId::from("call-2"),
                arguments_json_delta: r#"{"city":"Hang""#.into(),
            },
        },
        SequencedEvent {
            sequence: 6,
            event: ModelEvent::ToolCallDelta {
                call_id: CallId::from("call-2"),
                arguments_json_delta: r#"zhou"}"#.into(),
            },
        },
        SequencedEvent {
            sequence: 7,
            event: ModelEvent::UsageUpdated {
                usage: Usage::new(120, 30, 10, 0),
            },
        },
        SequencedEvent {
            sequence: 8,
            event: ModelEvent::Completed {
                response: Box::new(GenerateResponse {
                    invocation_id: InvocationId::from("inv-1"),
                    content: vec![
                        runtime_core::ContentBlock::Reasoning(ReasoningBlock {
                            text: "用户想查天气".into(),
                            provider_payload: Some(serde_json::json!({"sig": "x"})),
                        }),
                        runtime_core::ContentBlock::ToolCall(ToolCallBlock {
                            call_id: CallId::from("call-2"),
                            name: "get_weather".into(),
                            arguments_json: r#"{"city":"Hangzhou"}"#.into(),
                        }),
                        runtime_core::ContentBlock::Text(TextBlock::new("好的")),
                    ],
                    stop_reason: StopReason::ToolUse,
                    usage: Usage::new(120, 30, 10, 0),
                    provider_context: Default::default(),
                }),
            },
        },
    ];

    let mut validator = SequenceValidator::new();
    let json_lines: Vec<String> = events
        .iter()
        .map(|e| {
            validator
                .check(e.sequence)
                .expect("sequence must be monotonic");
            serde_json::to_string(e).unwrap()
        })
        .collect();

    let parsed: Vec<SequencedEvent> = json_lines
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(parsed, events);

    // 3. 终结事件恰好一个，且为 Completed
    let terminals: Vec<_> = parsed.iter().filter(|e| e.event.is_terminal()).collect();
    assert_eq!(terminals.len(), 1);
    match &terminals[0].event {
        ModelEvent::Completed { response } => {
            assert_eq!(response.stop_reason, StopReason::ToolUse);
            assert_eq!(response.text_content(), "好的");
            assert_eq!(response.tool_calls().count(), 1);
        }
        other => panic!("expected Completed, got {other:?}"),
    }

    // 4. 失败场景：partial() 必须可取回部分结果（总案 §25.1）
    let mut inv = Invocation::new("inv-2", request, 1000);
    inv.transition(InvocationState::Running).unwrap();
    inv.transition(InvocationState::Failed).unwrap();
    let partial = PartialOutput {
        content: vec![runtime_core::ContentBlock::Text(TextBlock::new(
            "已经生成的开头",
        ))],
        usage: Some(Usage::new(120, 5, 2, 0)),
        terminal_error: Some(ModelError::Overloaded(ErrorDetail::new(
            "provider overloaded",
        ))),
    };
    assert!(!partial.is_empty());
    assert_eq!(
        partial.terminal_error.as_ref().unwrap().kind_name(),
        "overloaded"
    );
}
