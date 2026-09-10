"""settings_state：支持从已保存配置恢复（restore），并跟踪密钥是否已保存。"""
import io

P = r"C:\个人文件\API\model-runtime\runtime-ui\src\settings_state.rs"
s = io.open(P, encoding="utf-8").read()

# 1) 字段：是否已有保存的密钥
s = s.replace(
    """    /// 模型发现的结果计数（用于显示"已获取 N 个"）。
    discovered_count: usize,
}""",
    """    /// 模型发现的结果计数（用于显示"已获取 N 个"）。
    discovered_count: usize,
    /// 凭据存储里是否已有该厂商的密钥（界面显示"已保存密钥"，
    /// 但**拿不到密钥值**，§32）。
    has_saved_key: bool,
}""",
)
s = s.replace(
    """            context_override_input: String::new(),
            probed_context: None,
            discovered_count: 0,
        };""",
    """            context_override_input: String::new(),
            probed_context: None,
            discovered_count: 0,
            has_saved_key: false,
        };""",
)

# 2) restore 方法
old_setter = """    pub fn set_protocol(&mut self, protocol: ProtocolKind) {"""
new_setter = """    /// 从已保存的配置恢复界面（启动时调用）。
    ///
    /// 未知的 provider/protocol 一律忽略而不是报错——配置文件可能来自
    /// 更新前的版本，能恢复多少恢复多少。
    pub fn restore(&mut self, saved: &crate::backend::SavedSettings) {
        if crate::preset::preset_by_id(&saved.provider).is_some() {
            // 只切 provider（会带出默认协议与地址），随后按保存值覆盖
            self.select_provider(&saved.provider);
        }
        if let Some(protocol) = protocol_from_slug(&saved.protocol) {
            self.set_protocol(protocol);
        }
        if !saved.endpoint.trim().is_empty() {
            self.set_endpoint(saved.endpoint.clone());
        }
        if let Some(context) = saved.context_window {
            self.context_override_input = context.to_string();
        }
        if let Some(model_id) = &saved.model_id {
            self.selected_model = Some(model_id.clone());
        }
        self.has_saved_key = saved.has_api_key;
        // 恢复出来的配置视为"未改动的现状"：地址是默认就标默认，
        // 否则标自定义，避免界面提示与实际不符
    }

    /// 凭据存储里是否已有密钥。
    pub fn has_saved_key(&self) -> bool {
        self.has_saved_key
    }

    pub fn set_has_saved_key(&mut self, value: bool) {
        self.has_saved_key = value;
    }

    /// 界面显示用的"是否需要填密钥"：非免密、输入框为空、且没有已保存的密钥。
    pub fn needs_key_input(&self) -> bool {
        !self.keyless() && self.api_key.trim().is_empty() && !self.has_saved_key
    }

    /// 保存成功后调用：记住密钥已存在，并按需清空输入框（密钥不留在界面状态里）。
    pub fn mark_key_saved(&mut self) {
        if !self.api_key.trim().is_empty() {
            self.has_saved_key = true;
            self.api_key.clear();
        }
    }

    pub fn set_protocol(&mut self, protocol: ProtocolKind) {"""
assert old_setter in s, "set_protocol anchor not found"
s = s.replace(old_setter, new_setter, 1)

# 3) protocol slug → ProtocolKind
s = s.replace(
    """/// 协议 → 稳定字符串（写进 draft / 存盘）。
pub fn protocol_slug(protocol: ProtocolKind) -> &'static str {""",
    """/// 稳定字符串 → 协议（读盘用）。未知字符串返回 `None`（不猜）。
pub fn protocol_from_slug(slug: &str) -> Option<ProtocolKind> {
    match slug.trim().to_lowercase().as_str() {
        "openai_chat" => Some(ProtocolKind::OpenAiChat),
        "openai_responses" => Some(ProtocolKind::OpenAiResponses),
        "anthropic_messages" => Some(ProtocolKind::AnthropicMessages),
        "gemini" => Some(ProtocolKind::Gemini),
        "provider_native" => Some(ProtocolKind::ProviderNative),
        _ => None,
    }
}

/// 协议 → 稳定字符串（写进 draft / 存盘）。
pub fn protocol_slug(protocol: ProtocolKind) -> &'static str {""",
)

# 4) validate 里改用 needs_key_input
s = s.replace(
    """        if !self.keyless() && self.api_key.trim().is_empty() {
            // 允许为空（用户可能先存配置后填 Key），因此只作为提示而非阻断
            issues.push("validation.key.empty_hint");
        }""",
    """        if self.needs_key_input() {
            // 允许为空（用户可能先存配置后填 Key），因此只是提示而非阻断
            issues.push("validation.key.empty_hint");
        }""",
)

# 5) 测试
s = s.replace(
    """    #[test]
    fn protocol_slug_matches_adapter_names() {""",
    """    #[test]
    fn restore_brings_back_a_saved_configuration() {
        let mut state = SettingsState::new();
        state.restore(&crate::backend::SavedSettings {
            provider: "zhipu".into(),
            protocol: "openai_chat".into(),
            endpoint: "https://my-gateway.example/v1".into(),
            model_id: Some("glm-4.6".into()),
            context_window: Some(200_000),
            has_api_key: true,
        });
        assert_eq!(state.selected_provider_id(), "zhipu");
        assert_eq!(state.endpoint(), "https://my-gateway.example/v1");
        assert_eq!(state.selected_model(), Some("glm-4.6"));
        assert_eq!(state.context_override_input(), "200000");
        assert!(state.has_saved_key());
        // 已保存密钥 → 不再要求用户重新填写
        assert!(!state.needs_key_input());
    }

    #[test]
    fn restore_ignores_unknown_provider_and_protocol_without_failing() {
        // 配置文件可能来自旧版本：能恢复多少恢复多少，不报错
        let mut state = SettingsState::new();
        let before = state.selected_provider_id().to_string();
        state.restore(&crate::backend::SavedSettings {
            provider: "provider-that-was-removed".into(),
            protocol: "protocol-that-never-existed".into(),
            endpoint: String::new(),
            model_id: None,
            context_window: None,
            has_api_key: false,
        });
        assert_eq!(state.selected_provider_id(), before, "未知 provider 应忽略");
        assert!(state.selected_model().is_none());
    }

    #[test]
    fn mark_key_saved_clears_the_secret_from_ui_state() {
        let mut state = SettingsState::new();
        state.select_provider("deepseek");
        state.set_api_key("sk-SHOULD-NOT-STAY");
        state.mark_key_saved();
        assert!(state.has_saved_key());
        assert_eq!(state.api_key(), "", "保存后密钥不该留在界面状态里");
        assert!(!state.needs_key_input());
    }

    #[test]
    fn protocol_slug_round_trips() {
        for protocol in [
            ProtocolKind::OpenAiChat,
            ProtocolKind::OpenAiResponses,
            ProtocolKind::AnthropicMessages,
            ProtocolKind::Gemini,
        ] {
            assert_eq!(protocol_from_slug(protocol_slug(protocol)), Some(protocol));
        }
        assert_eq!(protocol_from_slug("nonsense"), None);
    }

    #[test]
    fn protocol_slug_matches_adapter_names() {""",
)

io.open(P, "w", encoding="utf-8", newline="\n").write(s)
print("settings_state.rs patched")
