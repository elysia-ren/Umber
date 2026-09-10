//! 端到端集成测试：贯穿 Model Intelligence → 协议 Adapter → Engine 的完整链路。
//!
//! 这是"宿主拿到的是什么"的最终验收：Discovery 建立 Deployment → Registry
//! 挂接 ModelProfile → 用户按 UISpec 配置 → Engine 驱动一次流式调用。

use std::sync::Arc;

use runtime_core::message::Message;
use runtime_core::request::GenerateRequest;
use runtime_core::response::StopReason;
use runtime_credential::{CredentialRef, CredentialStore, InMemoryCredentialStore, SecretString};
use runtime_engine::{run_invocation, CancelToken, RetryPolicy, TimeoutPolicy};
use runtime_model::capability::CapabilityKind;
use runtime_model::catalog::{Catalog, CatalogSource};
use runtime_model::deployment::{Deployment, Endpoint, ProtocolKind};
use runtime_model::model::ModelProfile;
use runtime_model::registry::ModelRegistry;
use runtime_ui::discovery::{DiscoverySession, UiModelEntry};
use runtime_ui::schema::{SettingsDraft, SettingsPage};
use runtime_ui::strings::Strings;

use runtime_protocol::{OpenAiChatAdapter, ScriptedTransport};
use runtime_provider::ProviderAdapter;

const SSE: &str = concat!(
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"杭州晴，28°C\"},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
    "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":6}}\n\n",
    "data: [DONE]\n\n",
);

#[test]
fn host_journey_from_settings_to_streamed_answer() {
    // 1. 宿主渲染配置页（UISpec）；用户按"厂商 DeepSeek + OpenAI Chat + 自填地址"填写
    let page = SettingsPage::provider_settings();
    let strings = Strings::builtin("zh-CN").unwrap();
    assert!(strings.has(&page.title_key));

    let mut draft = SettingsDraft::default();
    draft.values.insert("provider".into(), "deepseek".into());
    draft.values.insert("protocol".into(), "openai_chat".into());
    draft
        .values
        .insert("endpoint".into(), "https://my-gateway.example/v1".into());
    draft.values.insert("api_key".into(), "sk-user".into());
    assert!(page.validate(&draft).is_empty(), "合法配置不应有校验问题");

    // 2. 凭据进入 CredentialStore（引用式，不落配置 JSON）
    let credentials = InMemoryCredentialStore::new();
    let key_ref = CredentialRef::from("custom/my-gateway/api_key");
    credentials
        .set(&key_ref, SecretString::new("sk-user"))
        .unwrap();

    // 3. Discovery：该网关没有 /models → 失败不阻断，手动添加模型 ID（§36）
    let mut session = DiscoverySession::new();
    session.begin().unwrap();
    session.fail("discovery.no_model_list").unwrap();
    assert!(strings.has("discovery.no_model_list"));
    session.add_manual_model("deepseek-chat").unwrap();
    let discovered = match session.state() {
        runtime_ui::DiscoveryState::Done { models } => models.clone(),
        other => panic!("unexpected discovery state {other:?}"),
    };
    assert_eq!(discovered.len(), 1);

    // 4. Endpoint 覆盖 Preset（§34）：Provider=DeepSeek，Endpoint=自定义网关
    let endpoint = Endpoint {
        id: "ep-gateway".into(),
        provider_id: "deepseek".into(),
        url: draft.values["endpoint"].clone(),
    };
    let model_id = discovered[0].model_id.clone();
    let deployment = Deployment {
        id: format!("deepseek/custom/openai_chat/{model_id}").into(),
        endpoint_id: endpoint.id.clone(),
        protocol: ProtocolKind::OpenAiChat,
        model_id: model_id.clone(),
    };

    // 5. Registry：Catalog 有身份级知识 → 绑定为 Deployment 级 ModelProfile（§37）
    let catalog = Catalog {
        format_version: 1,
        generated_at_unix: 1,
        sources: vec![CatalogSource {
            name: "models_dev".into(),
            snapshot: "2026-09-10".into(),
            license: "MIT".into(),
            url: "https://models.dev".into(),
        }],
        identities: vec![],
        entries: vec![ModelProfile {
            identity: runtime_model::identity::ModelIdentity {
                canonical_id: model_id.clone(),
                family: "deepseek".into(),
                version: None,
                organization: Some("deepseek-ai".into()),
                aliases: vec![],
            },
            deployment: None,
            display_name: "DeepSeek Chat".into(),
            description: String::new(),
            capabilities: Default::default(),
            limits: runtime_model::model::ModelLimits {
                context_window: Some(64_000),
                max_output_tokens: None,
            },
            modalities: Default::default(),
            reasoning: Default::default(),
            tool_support: Default::default(),
            structured_output: Default::default(),
            parameter_support: Default::default(),
            pricing: None,
            compatibility: Default::default(),
            evidence: vec![],
        }],
        deployments: vec![],
    };
    let mut registry = ModelRegistry::from_catalog(&catalog).unwrap();
    let info = registry.register_deployment(deployment.clone(), Some(&catalog));
    assert_eq!(info.limits.context_window, Some(64_000));
    // Catalog 未记录的能力仍是 Unknown，不假定兼容（§15）
    assert_eq!(
        info.capability_status(CapabilityKind::ToolCall),
        runtime_model::capability::CapabilityStatus::Unknown
    );

    // 6. 调用：Adapter → Engine，全程无 Provider 特化分支
    let transport = Arc::new(ScriptedTransport::new());
    transport.enqueue_sse(200, SSE);
    let adapter = OpenAiChatAdapter::new(transport.clone());

    let request = GenerateRequest::new(deployment.id.clone(), vec![Message::user("杭州天气")]);
    let factory = {
        let endpoint = endpoint.clone();
        let deployment = deployment.clone();
        let request = request.clone();
        move || adapter.execute(&request, &endpoint, &deployment, &credentials, &key_ref)
    };
    let mut events = Vec::new();
    let outcome = run_invocation(
        &factory,
        &request,
        &CancelToken::new(),
        &TimeoutPolicy::default(),
        &RetryPolicy::default(),
        &mut |e| events.push(e),
    );

    // 7. 断言最终产物
    runtime_conformance::assert::check_monotonic(&events).unwrap();
    runtime_conformance::assert::check_single_terminal(&events).unwrap();
    let response = outcome.response.expect("must complete");
    assert_eq!(response.stop_reason, StopReason::EndTurn);
    assert_eq!(response.text_content(), "杭州晴，28°C");
    assert_eq!(response.usage.input_tokens, 12);

    // 8. 请求确实打到了用户填的自定义地址，且凭据在 header（脱敏记录可验证）
    let recorded = transport.last_request().unwrap();
    assert_eq!(
        recorded.url,
        "https://my-gateway.example/v1/chat/completions"
    );
    assert_eq!(recorded.headers[0].0, "content-type");
    assert!(recorded
        .headers
        .iter()
        .any(|(k, v)| k == "authorization" && v == "[REDACTED]"));

    // DiscoveredModel 也用于 UI 展示（形状对齐）
    let _ui_entry = UiModelEntry {
        model_id: discovered[0].model_id.clone(),
        display_name: None,
    };
}
