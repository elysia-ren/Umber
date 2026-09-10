//! M1 引擎集成测试：用 umber-conformance 的 fake provider 验证
//! 总案 §23–§31 的全部硬保证。

use std::time::Duration;

use umber_conformance::assert as cassert;
use umber_conformance::fake::{Script, ScriptedFactory};
use umber_core::content::TextBlock;
use umber_core::error::{ErrorDetail, ModelError, TimeoutKind};
use umber_core::event::{ModelEvent, SequencedEvent};
use umber_core::ids::{BlockId, InvocationId};
use umber_core::message::Message;
use umber_core::request::GenerateRequest;
use umber_core::response::{GenerateResponse, StopReason};
use umber_core::usage::Usage;
use umber_engine::{run_invocation, CancelToken, RetryPolicy, TimeoutPolicy};

fn request() -> GenerateRequest {
    GenerateRequest::new("dep-1", vec![Message::user("hi")])
}

fn text_started() -> ModelEvent {
    ModelEvent::TextStarted {
        block_id: BlockId::from("t1"),
    }
}

fn text_delta(s: &str) -> ModelEvent {
    ModelEvent::TextDelta {
        block_id: BlockId::from("t1"),
        delta: s.to_string(),
    }
}

fn completed(invocation_id: &str, text: &str) -> ModelEvent {
    ModelEvent::Completed {
        response: Box::new(GenerateResponse {
            invocation_id: InvocationId::from(invocation_id),
            content: vec![umber_core::ContentBlock::Text(TextBlock::new(text))],
            stop_reason: StopReason::EndTurn,
            usage: Usage::new(10, 5, 0, 0),
            provider_context: Default::default(),
        }),
    }
}

fn run(
    factory: &ScriptedFactory,
    timeouts: TimeoutPolicy,
    retry: RetryPolicy,
) -> (Vec<SequencedEvent>, umber_engine::InvocationOutcome) {
    let cancel = CancelToken::new();
    let mut events = Vec::new();
    let outcome = run_invocation(factory, &request(), &cancel, &timeouts, &retry, &mut |e| {
        events.push(e)
    });
    (events, outcome)
}

#[test]
fn happy_path_assembles_and_completes() {
    let factory = ScriptedFactory::new(
        Script::builder()
            .emit(text_started())
            .emit(text_delta("你好"))
            .emit(text_delta("，世界"))
            .emit(ModelEvent::TextEnded {
                block_id: BlockId::from("t1"),
            })
            .emit(ModelEvent::UsageUpdated {
                usage: Usage::new(10, 5, 0, 0),
            })
            .emit(completed("whatever", "你好，世界"))
            .build(),
    );
    let (events, outcome) = run(&factory, TimeoutPolicy::default(), RetryPolicy::default());

    cassert::check_monotonic(&events).unwrap();
    cassert::check_single_terminal(&events).unwrap();
    // sequence 从 0 连续密集签发；Started 由 Engine 合成
    assert_eq!(events[0].sequence, 0);
    assert!(matches!(events[0].event, ModelEvent::Started { .. }));
    assert_eq!(events.last().unwrap().sequence as usize, events.len() - 1);

    let response = outcome.response.expect("must complete");
    assert_eq!(response.stop_reason, StopReason::EndTurn);
    assert_eq!(outcome.partial.text(), "你好，世界");
}

#[test]
fn eof_with_content_synthesizes_provider_error() {
    let factory = ScriptedFactory::new(
        Script::builder()
            .emit(text_started())
            .emit(text_delta("partial"))
            .eof()
            .build(),
    );
    let (events, outcome) = run(&factory, TimeoutPolicy::default(), RetryPolicy::default());

    cassert::check_single_terminal(&events).unwrap();
    let terminal = cassert::terminal(&events).unwrap();
    match &terminal.event {
        ModelEvent::Failed { error } => {
            assert_eq!(error.kind_name(), "provider_error");
            assert!(!error.retryable());
        }
        other => panic!("expected Failed, got {other:?}"),
    }
    assert!(outcome.response.is_none());
    assert!(outcome.partial.terminal_error.is_some());
}

#[test]
fn immediate_eof_synthesizes_empty_response() {
    let factory = ScriptedFactory::new(Script::builder().eof().build());
    let (events, _) = run(&factory, TimeoutPolicy::default(), RetryPolicy::default());
    let terminal = cassert::terminal(&events).unwrap();
    assert!(matches!(
        &terminal.event,
        ModelEvent::Failed { error } if matches!(error, ModelError::EmptyResponse)
    ));
}

#[test]
fn retryable_transport_error_recovers_on_second_open() {
    let factory = ScriptedFactory::new(
        Script::builder()
            .emit(text_started())
            .emit(text_delta("ok"))
            .emit(completed("x", "ok"))
            .build(),
    )
    .with_open_failures(1, ModelError::NetworkError(ErrorDetail::new("conn reset")));
    let (events, outcome) = run(&factory, TimeoutPolicy::default(), RetryPolicy::default());

    cassert::check_single_terminal(&events).unwrap();
    assert!(outcome.response.is_some());
    // 只有一次 Started（重试不复播 Started）
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.event, ModelEvent::Started { .. }))
            .count(),
        1
    );
}

#[test]
fn no_retry_once_content_emitted() {
    let factory = ScriptedFactory::new(
        Script::builder()
            .emit(text_started())
            .emit(text_delta("partial"))
            .fail(ModelError::RateLimited {
                detail: ErrorDetail::new("429"),
                retry_after_ms: Some(1),
            })
            .build(),
    );
    let (events, outcome) = run(&factory, TimeoutPolicy::default(), RetryPolicy::default());

    cassert::check_single_terminal(&events).unwrap();
    let terminal = cassert::terminal(&events).unwrap();
    assert!(matches!(
        &terminal.event,
        ModelEvent::Failed { error } if error.kind_name() == "rate_limited"
    ));
    // 部分结果必须可取回（总案 §25.1）
    assert_eq!(outcome.partial.text(), "partial");
}

#[test]
fn open_failures_exhaust_retries() {
    let factory = ScriptedFactory::new(Script::builder().build())
        .with_open_failures(5, ModelError::NetworkError(ErrorDetail::new("down")));
    let (events, _) = run(&factory, TimeoutPolicy::default(), RetryPolicy::default());

    let terminal = cassert::terminal(&events).unwrap();
    assert!(matches!(
        &terminal.event,
        ModelEvent::Failed { error } if error.kind_name() == "network_error"
    ));
}

#[test]
fn idle_timeout_is_labeled_by_engine() {
    let factory = ScriptedFactory::new(
        Script::builder()
            .delay(Duration::from_millis(10_000))
            .emit(text_started())
            .build(),
    );
    let timeouts = TimeoutPolicy {
        first_token: Duration::from_millis(50),
        ..TimeoutPolicy::default()
    };
    let (events, _) = run(&factory, timeouts, RetryPolicy::default());

    let terminal = cassert::terminal(&events).unwrap();
    match &terminal.event {
        ModelEvent::Failed { error } => match error {
            ModelError::Timeout { kind, .. } => assert_eq!(*kind, TimeoutKind::FirstToken),
            other => panic!("expected timeout, got {other:?}"),
        },
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn total_timeout_synthesizes_failure() {
    let factory = ScriptedFactory::new(
        Script::builder()
            .delay(Duration::from_millis(5_000))
            .emit(text_started())
            .build(),
    );
    let timeouts = TimeoutPolicy {
        total: Some(Duration::from_millis(40)),
        ..TimeoutPolicy::default()
    };
    let (events, _) = run(&factory, timeouts, RetryPolicy::default());

    let terminal = cassert::terminal(&events).unwrap();
    assert!(matches!(
        &terminal.event,
        ModelEvent::Failed { error }
            if matches!(error, ModelError::Timeout { kind: TimeoutKind::Total, .. })
    ));
}

#[test]
fn precancelled_invocation_yields_cancelled_with_partial() {
    let factory = ScriptedFactory::new(
        Script::builder()
            .emit(text_started())
            .emit(completed("x", "done"))
            .build(),
    );
    let cancel = CancelToken::new();
    cancel.cancel();
    let mut events = Vec::new();
    let outcome = run_invocation(
        &factory,
        &request(),
        &cancel,
        &TimeoutPolicy::default(),
        &RetryPolicy::default(),
        &mut |e| events.push(e),
    );
    cassert::check_single_terminal(&events).unwrap();
    assert!(outcome.response.is_none());
    assert_eq!(outcome.partial.terminal_error, Some(ModelError::Cancelled));
}

#[test]
fn cancellation_mid_stream_interrupts_wait() {
    let factory = ScriptedFactory::new(
        Script::builder()
            .emit(text_started())
            .delay(Duration::from_secs(30))
            .emit(completed("x", "never"))
            .build(),
    );
    let cancel = CancelToken::new();
    let cancel2 = cancel.clone();
    let handle = std::thread::spawn(move || {
        let mut events = Vec::new();
        let outcome = run_invocation(
            &factory,
            &request(),
            &cancel2,
            &TimeoutPolicy::default(),
            &RetryPolicy::default(),
            &mut |e| events.push(e),
        );
        (events, outcome)
    });
    std::thread::sleep(Duration::from_millis(50));
    cancel.cancel();
    let (events, outcome) = handle.join().unwrap();

    cassert::check_single_terminal(&events).unwrap();
    assert!(matches!(
        cassert::terminal(&events).unwrap().event,
        ModelEvent::Cancelled
    ));
    assert_eq!(outcome.partial.text(), "");
    // 从发起到终结必须远小于 30s 的脚本延迟
}

#[test]
fn adapter_violation_started_is_ignored() {
    let factory = ScriptedFactory::new(
        Script::builder()
            .emit(ModelEvent::Started {
                invocation_id: InvocationId::from("forged"),
            })
            .emit(text_started())
            .emit(completed("x", "ok"))
            .build(),
    );
    let (events, outcome) = run(&factory, TimeoutPolicy::default(), RetryPolicy::default());

    assert!(matches!(events[0].event, ModelEvent::Started { .. }));
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.event, ModelEvent::Started { .. }))
            .count(),
        1,
        "Engine 的 Started 是唯一一份"
    );
    assert!(outcome.response.is_some());
}
