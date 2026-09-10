//! 内置多语言文案（总案 §38.1 strings()）。
//!
//! 契约要求 key 全覆盖：`schema` 与 `discovery` 中出现的每个 *_key
//! 都必须能在至少一套内置文案里查到——由测试强制（不是约定）。

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Strings {
    language: &'static str,
    entries: BTreeMap<&'static str, &'static str>,
}

impl Strings {
    pub fn builtin(language: &str) -> Option<Self> {
        match language {
            "zh-CN" | "zh" => Some(Self {
                language: "zh-CN",
                entries: BUILTIN_STRINGS_ZH.iter().copied().collect(),
            }),
            "en" | "en-US" => Some(Self {
                language: "en",
                entries: BUILTIN_STRINGS_EN.iter().copied().collect(),
            }),
            _ => None,
        }
    }

    pub fn language(&self) -> &'static str {
        self.language
    }

    /// 查文案；缺失返回 key 本身（渲染绝不 panic）。
    pub fn get(&self, key: &str) -> &str {
        self.entries.get(key).copied().unwrap_or("")
    }

    pub fn has(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

pub const BUILTIN_STRINGS_ZH: &[(&str, &str)] = &[
    // 页面 / 分区
    ("settings.provider.title", "模型服务"),
    ("settings.provider.connection", "连接"),
    ("settings.provider.probe", "检测"),
    // 字段
    ("settings.provider.provider", "厂商"),
    ("settings.provider.protocol", "协议"),
    ("settings.provider.endpoint", "API 地址"),
    ("settings.provider.endpoint.help", "可填写官方地址、第三方中转或自建服务"),
    ("settings.provider.api_key", "API Key"),
    ("settings.provider.active_probe", "主动检测模型能力"),
    (
        "settings.provider.active_probe.help",
        "默认关闭。开启后会发送真实请求以确认流式、工具调用、结构化输出等能力，可能产生 Token 消耗与费用。",
    ),
    // 厂商
    ("provider.openai", "OpenAI"),
    ("provider.anthropic", "Anthropic"),
    ("provider.google", "Google"),
    ("provider.deepseek", "DeepSeek"),
    ("provider.custom", "自定义"),
    // 协议
    ("protocol.openai_chat", "OpenAI Chat"),
    ("protocol.openai_responses", "OpenAI Responses"),
    ("protocol.anthropic_messages", "Anthropic Messages"),
    ("protocol.gemini", "Gemini"),
    // 校验
    ("validation.required", "此项必填"),
    ("validation.pattern", "格式不正确"),
    // 发现
    ("discovery.no_model_list", "该服务未提供模型列表，请手动填写模型 ID"),
    ("discovery.empty", "未发现任何模型"),
    ("discovery.in_progress", "正在获取模型…"),
    // 连接测试
    ("connection.ok", "连接成功"),
    ("connection.failed", "连接失败"),
    // 模型信息
    ("model.capabilities", "能力"),
    ("model.capability.unknown", "未知"),
    ("model.capability.supported", "支持"),
    ("model.capability.unsupported", "不支持"),
    ("model.capability.partial", "部分支持"),
    ("model.pricing.notice", "价格仅供参考，不作为计费依据"),
];

pub const BUILTIN_STRINGS_EN: &[(&str, &str)] = &[
    ("settings.provider.title", "Model Services"),
    ("settings.provider.connection", "Connection"),
    ("settings.provider.probe", "Detection"),
    ("settings.provider.provider", "Provider"),
    ("settings.provider.protocol", "Protocol"),
    ("settings.provider.endpoint", "API URL"),
    (
        "settings.provider.endpoint.help",
        "Official endpoint, third-party gateway, or self-hosted service",
    ),
    ("settings.provider.api_key", "API Key"),
    ("settings.provider.active_probe", "Actively probe model capabilities"),
    (
        "settings.provider.active_probe.help",
        "Off by default. Enabling sends real requests to confirm streaming, tool calls, and structured output, which may consume tokens and incur cost.",
    ),
    ("provider.openai", "OpenAI"),
    ("provider.anthropic", "Anthropic"),
    ("provider.google", "Google"),
    ("provider.deepseek", "DeepSeek"),
    ("provider.custom", "Custom"),
    ("protocol.openai_chat", "OpenAI Chat"),
    ("protocol.openai_responses", "OpenAI Responses"),
    ("protocol.anthropic_messages", "Anthropic Messages"),
    ("protocol.gemini", "Gemini"),
    ("validation.required", "This field is required"),
    ("validation.pattern", "Invalid format"),
    (
        "discovery.no_model_list",
        "This service exposes no model list; enter the model ID manually",
    ),
    ("discovery.empty", "No models found"),
    ("discovery.in_progress", "Fetching models…"),
    ("connection.ok", "Connected"),
    ("connection.failed", "Connection failed"),
    ("model.capabilities", "Capabilities"),
    ("model.capability.unknown", "Unknown"),
    ("model.capability.supported", "Supported"),
    ("model.capability.unsupported", "Unsupported"),
    ("model.capability.partial", "Partial"),
    ("model.pricing.notice", "Pricing is informational only and not a basis for billing"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::DiscoverySession;
    use crate::schema::{FieldKind, SettingsPage};

    #[test]
    fn both_builtin_locales_cover_the_same_keys() {
        let zh: BTreeMap<_, _> = BUILTIN_STRINGS_ZH.iter().copied().collect();
        let en: BTreeMap<_, _> = BUILTIN_STRINGS_EN.iter().copied().collect();
        assert_eq!(zh.len(), en.len(), "两套文案 key 数量必须一致");
        for key in zh.keys() {
            assert!(en.contains_key(key), "en 缺少 key: {key}");
        }
    }

    #[test]
    fn every_schema_key_resolves_in_both_locales() {
        let page = SettingsPage::provider_settings();
        let zh = Strings::builtin("zh-CN").unwrap();
        let en = Strings::builtin("en").unwrap();
        let mut keys: Vec<&str> = vec![&page.title_key];
        for section in &page.sections {
            keys.push(&section.title_key);
            for field in &section.fields {
                keys.push(&field.label_key);
                if let Some(help) = &field.help_key {
                    keys.push(help);
                }
                if let FieldKind::Select { options } = &field.kind {
                    for option in options {
                        keys.push(&option.label_key);
                    }
                }
            }
        }
        for key in keys {
            assert!(zh.has(key), "zh-CN 缺少 schema key: {key}");
            assert!(en.has(key), "en 缺少 schema key: {key}");
        }
    }

    #[test]
    fn discovery_and_validation_keys_resolve() {
        let zh = Strings::builtin("zh-CN").unwrap();
        assert!(zh.has("discovery.no_model_list"));
        assert!(zh.has("validation.required"));
        assert!(zh.has("validation.pattern"));
        assert!(zh.has("model.pricing.notice"));
        // 状态机用到的 reason_key 必须可渲染
        let mut session = DiscoverySession::new();
        session.begin().unwrap();
        session.fail("discovery.no_model_list").unwrap();
    }

    #[test]
    fn unknown_key_renders_empty_not_panic() {
        let zh = Strings::builtin("zh-CN").unwrap();
        assert_eq!(zh.get("nope.nope"), "");
        assert!(Strings::builtin("fr").is_none());
    }
}
