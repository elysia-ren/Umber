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
    // 模型默认值（思考强度，§21.1）
    ("settings.provider.model_defaults", "模型默认值"),
    ("settings.provider.reasoning_effort", "思考强度"),
    (
        "settings.provider.reasoning_effort.help",
        "按请求可再覆盖。实际生效档位取决于模型支持情况，不支持时由 Runtime 就近降级。",
    ),
    ("reasoning.minimal", "最小"),
    ("reasoning.low", "低"),
    ("reasoning.medium", "中"),
    ("reasoning.high", "高"),
    // 发现 / 连接测试（UISpec 应用层使用）
    ("discovery.title", "模型获取"),
    ("discovery.auto", "自动获取模型"),
    ("discovery.manual.add", "手动填写模型 ID"),
    ("discovery.add", "添加"),
    ("connection.testing", "正在测试连接…"),
    ("connection.latency", "延迟"),
    ("settings.save", "保存"),
    ("settings.saved", "已保存"),
    ("models.none", "尚未选择模型"),
    // 向导
    ("wizard.title", "模型服务设置"),
    ("wizard.step.provider", "选择厂商"),
    ("wizard.step.credentials", "填写密钥"),
    ("wizard.step.model", "选择模型"),
    ("wizard.next", "下一步"),
    ("wizard.back", "上一步"),
    ("wizard.done", "完成"),
    ("wizard.preset.note", "选择后会自动带出协议与官方地址，可自行修改"),
    ("wizard.get_key", "获取 API Key"),
    ("wizard.endpoint.reset", "恢复官方地址"),
    ("wizard.endpoint.custom", "自定义地址"),
    ("wizard.key.optional", "免密服务可留空"),
    ("wizard.models.recommended", "推荐模型"),
    ("wizard.models.discovered", "已获取模型"),
    ("wizard.models.empty", "还没获取模型，可直接选推荐或手动填写"),
    ("wizard.advanced", "高级选项"),
    ("reasoning.effort.unknown", "未知（该服务未提供档位信息）"),
    // 向导校验
    ("validation.endpoint.required", "请填写 API 地址"),
    ("validation.endpoint.scheme", "地址需以 http:// 或 https:// 开头"),
    ("validation.model.required", "请选择一个模型"),
    // 厂商说明
    ("preset.note.deepseek", "DeepSeek 官方，提供 OpenAI 兼容与 Anthropic 兼容两种端点"),
    ("preset.note.openai", "OpenAI 官方，支持 Chat Completions 与 Responses"),
    ("preset.note.anthropic", "Anthropic 官方 Claude 系列"),
    ("preset.note.google", "Google Gemini 官方"),
    ("preset.note.custom", "自建服务、第三方中转或本地推理（如 Ollama、LM Studio）"),
    // 面向用户的协议名（不用开发者术语）
    ("protocol_label.openai_chat", "OpenAI 兼容"),
    ("protocol_label.openai_responses", "OpenAI Responses"),
    ("protocol_label.anthropic_messages", "Anthropic 兼容"),
    ("protocol_label.gemini", "Google Gemini"),
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
    // ---- 两栏设置界面 ----
    ("providers.search", "搜索厂商…"),
    ("providers.none", "没有匹配的厂商"),
    ("providers.docs", "文档"),
    ("providers.keyless", "免密"),
    ("providers.request_to", "请求将发送到"),
    ("providers.models", "模型列表"),
    ("providers.refresh_models", "刷新模型列表"),
    ("category.official", "官方直连"),
    ("category.china", "国内厂商"),
    ("category.gateway", "聚合与中转"),
    ("category.local", "本地部署"),
    ("category.custom", "自定义"),
    ("connection.test", "测试连接"),
    ("model.data.absent", "目录中没有该模型的数据（仍可正常使用）"),
    ("model.select_hint", "选择上方任一模型，查看能力、价格与上下文"),
    ("model.capability.unlisted", "目录未记录该能力字段"),
    ("model.pricing", "价格"),
    ("model.price.input", "输入"),
    ("model.price.output", "输出"),
    ("model.price.cached", "缓存"),
    ("model.context_window", "上下文窗口"),
    ("model.context_window.hint", "留空使用自动探测值"),
    ("model.context.probed", "自动探测"),
    ("model.context.catalog", "目录"),
    ("model.context.overridden", "覆盖"),
    ("model.context.effective", "生效"),
    ("model.context.conflict", "（与其他来源不一致）"),
    ("model.context.unknown", "尚无数据：未探测且目录中无该模型"),
    // 新增厂商
    ("provider.xai", "xAI Grok"),
    ("provider.mistral", "Mistral"),
    ("provider.groq", "Groq"),
    ("provider.perplexity", "Perplexity"),
    ("provider.zhipu", "智谱 GLM"),
    ("provider.zai", "Z.AI（智谱国际）"),
    ("provider.moonshot", "Kimi / Moonshot"),
    ("provider.dashscope", "阿里云百炼"),
    ("provider.qianfan", "百度智能云千帆"),
    ("provider.hunyuan", "腾讯混元"),
    ("provider.minimax", "MiniMax"),
    ("provider.stepfun", "阶跃星辰 StepFun"),
    ("provider.sensenova", "商汤日日新"),
    ("provider.ark", "火山方舟"),
    ("provider.siliconflow", "硅基流动 SiliconFlow"),
    ("provider.modelscope", "魔搭 ModelScope"),
    ("provider.openrouter", "OpenRouter"),
    ("provider.together", "Together AI"),
    ("provider.fireworks", "Fireworks AI"),
    ("provider.nvidia", "NVIDIA NIM"),
    ("provider.cerebras", "Cerebras"),
    ("provider.ollama", "Ollama（本地）"),
    ("provider.lmstudio", "LM Studio（本地）"),
    ("provider.vllm", "vLLM（本地）"),
    ("validation.key.empty_hint", "尚未填写密钥"),
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
    ("settings.provider.model_defaults", "Model Defaults"),
    ("settings.provider.reasoning_effort", "Reasoning Effort"),
    (
        "settings.provider.reasoning_effort.help",
        "Can be overridden per request. The effective level depends on model support; the Runtime downgrades to the nearest supported level.",
    ),
    ("reasoning.minimal", "Minimal"),
    ("reasoning.low", "Low"),
    ("reasoning.medium", "Medium"),
    ("reasoning.high", "High"),
    ("discovery.title", "Model Discovery"),
    ("discovery.auto", "Fetch models"),
    ("discovery.manual.add", "Enter model ID manually"),
    ("discovery.add", "Add"),
    ("connection.testing", "Testing connection…"),
    ("connection.latency", "Latency"),
    ("settings.save", "Save"),
    ("settings.saved", "Saved"),
    ("models.none", "No model selected yet"),
    ("wizard.title", "Model Service Setup"),
    ("wizard.step.provider", "Choose provider"),
    ("wizard.step.credentials", "Enter key"),
    ("wizard.step.model", "Choose model"),
    ("wizard.next", "Next"),
    ("wizard.back", "Back"),
    ("wizard.done", "Done"),
    ("wizard.preset.note", "Protocol and official endpoint are filled in automatically; you can change them"),
    ("wizard.get_key", "Get API key"),
    ("wizard.endpoint.reset", "Reset to official endpoint"),
    ("wizard.endpoint.custom", "Custom endpoint"),
    ("wizard.key.optional", "May be left empty for keyless services"),
    ("wizard.models.recommended", "Recommended models"),
    ("wizard.models.discovered", "Discovered models"),
    ("wizard.models.empty", "No models fetched yet — pick a recommendation or type an ID"),
    ("wizard.advanced", "Advanced"),
    ("reasoning.effort.unknown", "Unknown (service provides no level information)"),
    ("validation.endpoint.required", "Please enter an API URL"),
    ("validation.endpoint.scheme", "URL must start with http:// or https://"),
    ("validation.model.required", "Please choose a model"),
    ("preset.note.deepseek", "Official DeepSeek; offers OpenAI-compatible and Anthropic-compatible endpoints"),
    ("preset.note.openai", "Official OpenAI; Chat Completions and Responses"),
    ("preset.note.anthropic", "Official Anthropic Claude models"),
    ("preset.note.google", "Official Google Gemini"),
    ("preset.note.custom", "Self-hosted, gateway, or local inference (Ollama, LM Studio, …)"),
    ("protocol_label.openai_chat", "OpenAI compatible"),
    ("protocol_label.openai_responses", "OpenAI Responses"),
    ("protocol_label.anthropic_messages", "Anthropic compatible"),
    ("protocol_label.gemini", "Google Gemini"),
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
    ("providers.search", "Search providers…"),
    ("providers.none", "No matching provider"),
    ("providers.docs", "Docs"),
    ("providers.keyless", "No key needed"),
    ("providers.request_to", "Requests go to"),
    ("providers.models", "Models"),
    ("providers.refresh_models", "Refresh models"),
    ("category.official", "Official"),
    ("category.china", "China"),
    ("category.gateway", "Gateways"),
    ("category.local", "Local"),
    ("category.custom", "Custom"),
    ("connection.test", "Test connection"),
    ("model.data.absent", "No catalog data for this model (still usable)"),
    ("model.select_hint", "Pick a model above to see capabilities, pricing and context"),
    ("model.capability.unlisted", "The catalog does not record this capability"),
    ("model.pricing", "Pricing"),
    ("model.price.input", "in"),
    ("model.price.output", "out"),
    ("model.price.cached", "cached"),
    ("model.context_window", "Context window"),
    ("model.context_window.hint", "Leave empty to use the probed value"),
    ("model.context.probed", "probed"),
    ("model.context.catalog", "catalog"),
    ("model.context.overridden", "override"),
    ("model.context.effective", "effective"),
    ("model.context.conflict", "(differs from other sources)"),
    ("model.context.unknown", "No data yet: not probed and not in the catalog"),
    ("provider.xai", "xAI Grok"),
    ("provider.mistral", "Mistral"),
    ("provider.groq", "Groq"),
    ("provider.perplexity", "Perplexity"),
    ("provider.zhipu", "Zhipu GLM"),
    ("provider.zai", "Z.AI"),
    ("provider.moonshot", "Kimi / Moonshot"),
    ("provider.dashscope", "Alibaba Bailian"),
    ("provider.qianfan", "Baidu Qianfan"),
    ("provider.hunyuan", "Tencent Hunyuan"),
    ("provider.minimax", "MiniMax"),
    ("provider.stepfun", "StepFun"),
    ("provider.sensenova", "SenseNova"),
    ("provider.ark", "Volcengine Ark"),
    ("provider.siliconflow", "SiliconFlow"),
    ("provider.modelscope", "ModelScope"),
    ("provider.openrouter", "OpenRouter"),
    ("provider.together", "Together AI"),
    ("provider.fireworks", "Fireworks AI"),
    ("provider.nvidia", "NVIDIA NIM"),
    ("provider.cerebras", "Cerebras"),
    ("provider.ollama", "Ollama (local)"),
    ("provider.lmstudio", "LM Studio (local)"),
    ("provider.vllm", "vLLM (local)"),
    ("validation.key.empty_hint", "No API key yet"),
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
    fn app_layer_keys_resolve_in_both_locales() {
        // egui 参考实现使用的全部 key（schema 覆盖测试之外）
        const APP_KEYS: &[&str] = &[
            "settings.provider.model_defaults",
            "settings.provider.reasoning_effort",
            "settings.provider.reasoning_effort.help",
            "reasoning.minimal",
            "reasoning.low",
            "reasoning.medium",
            "reasoning.high",
            "discovery.title",
            "discovery.auto",
            "discovery.manual.add",
            "discovery.add",
            "discovery.in_progress",
            "connection.testing",
            "connection.ok",
            "connection.failed",
            "connection.latency",
            "settings.save",
            "settings.saved",
            "models.none",
            "model.capabilities",
            "model.capability.unknown",
        ];
        for language in ["zh-CN", "en"] {
            let strings = Strings::builtin(language).unwrap();
            for key in APP_KEYS {
                assert!(strings.has(key), "{language} 缺少 key: {key}");
            }
        }
        // 思考强度选项与 schema 中 Select options 的 value 一致（§21.1）
        let page = SettingsPage::provider_settings();
        let field = page
            .sections
            .iter()
            .flat_map(|s| s.fields.iter())
            .find(|f| f.id == "default_reasoning_effort")
            .expect("reasoning effort field must exist");
        let values: Vec<&str> = match &field.kind {
            FieldKind::Select { options } => options.iter().map(|o| o.value.as_str()).collect(),
            _ => panic!("reasoning effort must be a select"),
        };
        assert_eq!(values, ["minimal", "low", "medium", "high"]);
        assert_eq!(field.default.as_deref(), Some("medium"));
    }

    #[test]
    fn wizard_keys_resolve_in_both_locales() {
        use crate::preset::BUILTIN_PRESETS;

        let mut keys: Vec<&str> = vec!["wizard.title"];
        keys.extend([
            "wizard.next",
            "wizard.back",
            "wizard.done",
            "wizard.preset.note",
            "wizard.get_key",
            "wizard.endpoint.reset",
            "wizard.key.optional",
            "wizard.models.recommended",
            "wizard.models.discovered",
            "wizard.models.empty",
            "wizard.advanced",
            "reasoning.effort.unknown",
            "validation.endpoint.required",
            "validation.endpoint.scheme",
            "validation.model.required",
        ]);
        // 每个 Preset 的说明文案 + 每个协议的面向用户名称
        for preset in BUILTIN_PRESETS {
            keys.push(preset.name_key);
            keys.push(preset.subtitle_key());
            for offering in preset.offerings {
                keys.push(offering.protocol_label_key);
            }
        }

        for language in ["zh-CN", "en"] {
            let strings = Strings::builtin(language).unwrap();
            for key in &keys {
                assert!(strings.has(key), "{language} 缺少 key: {key}");
            }
        }
    }

    #[test]
    fn unknown_key_renders_empty_not_panic() {
        let zh = Strings::builtin("zh-CN").unwrap();
        assert_eq!(zh.get("nope.nope"), "");
        assert!(Strings::builtin("fr").is_none());
    }
}
