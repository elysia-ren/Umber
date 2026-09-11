//! Provider Preset：厂商预置（总案 §7 §31）。
//!
//! 设计原则（按实际使用修正过）：
//!
//! 1. **不硬编码模型名**。早期版本在预置里写死了 `deepseek-chat`、`moonshot-v1-8k`
//!    这类名字，结果是用户看到一年前的模型。模型清单必须来自
//!    ① 服务商 `/models` 实时拉取 ② 随包 Catalog 按厂商匹配 ③ 用户手填。
//!    预置只提供"去哪里找"（`catalog_provider_ids`），不提供"有哪些"。
//! 2. **Provider ≠ Protocol**（§6）：绝大多数厂商说的是 openai_chat，
//!    协议对用户是次要显示项。
//! 3. 分类顺序把**国内厂商放最前**——主要使用者在国内。
//! 4. Preset 只是默认值，端点永不被锁死（§34）。

use umber_model::deployment::ProtocolKind;

/// 厂商分类。`presets_by_category` 的输出顺序即界面分组顺序。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProviderCategory {
    /// 国内模型厂商与云厂商（界面第一组）。
    China,
    /// 海外官方直连。
    Official,
    /// 聚合与中转。
    Gateway,
    /// 本地部署（免密）。
    Local,
    /// 完全自定义。
    Custom,
}

impl ProviderCategory {
    pub fn title_key(self) -> &'static str {
        match self {
            ProviderCategory::China => "category.china",
            ProviderCategory::Official => "category.official",
            ProviderCategory::Gateway => "category.gateway",
            ProviderCategory::Local => "category.local",
            ProviderCategory::Custom => "category.custom",
        }
    }
}

/// 一个厂商在一种协议下的服务 offering。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderOffering {
    pub protocol: ProtocolKind,
    /// 官方默认端点（用户可改，§34）。
    pub default_endpoint: &'static str,
    /// 面向用户的协议名称 i18n key（不用开发者术语）。
    pub protocol_label_key: &'static str,
}

impl ProviderOffering {
    const fn chat(endpoint: &'static str) -> Self {
        Self {
            protocol: ProtocolKind::OpenAiChat,
            default_endpoint: endpoint,
            protocol_label_key: "protocol_label.openai_chat",
        }
    }

    const fn responses(endpoint: &'static str) -> Self {
        Self {
            protocol: ProtocolKind::OpenAiResponses,
            default_endpoint: endpoint,
            protocol_label_key: "protocol_label.openai_responses",
        }
    }

    const fn anthropic(endpoint: &'static str) -> Self {
        Self {
            protocol: ProtocolKind::AnthropicMessages,
            default_endpoint: endpoint,
            protocol_label_key: "protocol_label.anthropic_messages",
        }
    }

    const fn gemini(endpoint: &'static str) -> Self {
        Self {
            protocol: ProtocolKind::Gemini,
            default_endpoint: endpoint,
            protocol_label_key: "protocol_label.gemini",
        }
    }
}

/// 厂商预置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderPreset {
    pub id: &'static str,
    pub name_key: &'static str,
    pub category: ProviderCategory,
    /// 是否无需 API Key（本地部署）。
    pub keyless: bool,
    /// 申请 / 管理 API Key 的页面。
    pub key_url: Option<&'static str>,
    /// 官方文档。
    pub doc_url: Option<&'static str>,
    /// 卡片徽标用的短标识（我们没有各家 logo 授权）。
    pub badge: &'static str,
    /// 在随包 Catalog 中匹配该厂商的 provider 键
    /// （实测来自上游目录的 provider 标识，如 `zhipuai` / `moonshotai`）。
    /// 用于"推荐模型"——**不是硬编码模型名，而是按厂商查目录**。
    pub catalog_provider_ids: &'static [&'static str],
    pub offerings: &'static [ProviderOffering],
}

impl ProviderPreset {
    pub fn default_protocol(&self) -> ProtocolKind {
        self.offerings[0].protocol
    }

    pub fn offering(&self, protocol: ProtocolKind) -> Option<&ProviderOffering> {
        self.offerings.iter().find(|o| o.protocol == protocol)
    }

    pub fn subtitle_key(&self) -> &'static str {
        self.category.title_key()
    }

    /// 该厂商是否有可查的目录数据来源（本地部署与自定义没有）。
    pub fn has_catalog_source(&self) -> bool {
        !self.catalog_provider_ids.is_empty()
    }
}

/// 内置厂商预置。**国内厂商在前**，各组内按常见程度排序。
pub const BUILTIN_PRESETS: &[ProviderPreset] = &[
    // ==================== 国内厂商 ====================
    ProviderPreset {
        id: "deepseek",
        name_key: "provider.deepseek",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://platform.deepseek.com/api_keys"),
        doc_url: Some("https://api-docs.deepseek.com"),
        badge: "D",
        catalog_provider_ids: &["deepseek"],
        offerings: &[
            // 官方 base_url 表给的基准是 https://api.deepseek.com（无 /v1）。
            // /v1 是可用别名，实测两者都通；这里跟官方文档保持一致。
            ProviderOffering::chat("https://api.deepseek.com"),
            // 官方 Responses API（实测 POST /responses -> 200）
            ProviderOffering::responses("https://api.deepseek.com"),
            ProviderOffering::anthropic("https://api.deepseek.com/anthropic"),
        ],
    },
    ProviderPreset {
        id: "zhipu",
        name_key: "provider.zhipu",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://open.bigmodel.cn/usercenter/apikeys"),
        doc_url: Some("https://open.bigmodel.cn/dev/api"),
        badge: "智",
        catalog_provider_ids: &["zhipuai", "zhipuai-coding-plan"],
        offerings: &[
            ProviderOffering::chat("https://open.bigmodel.cn/api/paas/v4"),
            // 官方「Claude API 兼容」页：base 为 https://open.bigmodel.cn/api/anthropic
            ProviderOffering::anthropic("https://open.bigmodel.cn/api/anthropic"),
        ],
    },
    ProviderPreset {
        id: "dashscope",
        name_key: "provider.dashscope",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://bailian.console.aliyun.com/?apiKey=1"),
        doc_url: Some("https://help.aliyun.com/zh/model-studio/"),
        badge: "阿",
        catalog_provider_ids: &["alibaba", "alibaba-cn"],
        offerings: &[
            ProviderOffering::chat("https://dashscope.aliyuncs.com/compatible-mode/v1"),
            // 官方 Claude Code 文档给出的 Anthropic 兼容端点
            ProviderOffering::anthropic("https://dashscope.aliyuncs.com/apps/anthropic"),
        ],
    },
    ProviderPreset {
        id: "moonshot",
        name_key: "provider.moonshot",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://platform.moonshot.cn/console/api-keys"),
        doc_url: Some("https://platform.moonshot.cn/docs"),
        badge: "K",
        catalog_provider_ids: &["moonshotai", "moonshotai-cn"],
        offerings: &[
            ProviderOffering::chat("https://api.moonshot.cn/v1"),
            ProviderOffering::responses("https://api.moonshot.cn/v1"),
            // 官方文档：Anthropic 兼容 base 为 https://api.moonshot.cn/anthropic
            ProviderOffering::anthropic("https://api.moonshot.cn/anthropic"),
        ],
    },
    ProviderPreset {
        id: "ark",
        name_key: "provider.ark",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey"),
        doc_url: Some("https://www.volcengine.com/docs/82379"),
        badge: "火",
        catalog_provider_ids: &["volcengine"],
        offerings: &[
            ProviderOffering::chat("https://ark.cn-beijing.volces.com/api/v3"),
            ProviderOffering::responses("https://ark.cn-beijing.volces.com/api/v3"),
            // 官方 Messages（Anthropic 兼容）端点，非 Coding Plan 专属
            ProviderOffering::anthropic("https://ark.cn-beijing.volces.com/api/compatible"),
        ],
    },
    ProviderPreset {
        id: "minimax",
        name_key: "provider.minimax",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://platform.minimaxi.com/user-center/basic-information/interface-key"),
        doc_url: Some("https://platform.minimaxi.com/document"),
        badge: "mm",
        catalog_provider_ids: &["minimax", "minimax-cn"],
        offerings: &[
            // 国内站：api.minimax.chat 已过期，官方文档现行域名为 api.minimax.cn
            // （国际站为 api.minimax.io，见 docs 的国内/国际两套）
            ProviderOffering::chat("https://api.minimax.cn/v1"),
            ProviderOffering::responses("https://api.minimax.cn/v1"),
            ProviderOffering::anthropic("https://api.minimax.cn/anthropic"),
        ],
    },
    ProviderPreset {
        id: "stepfun",
        name_key: "provider.stepfun",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://platform.stepfun.com/interface-key"),
        doc_url: Some("https://platform.stepfun.com/docs"),
        badge: "阶",
        catalog_provider_ids: &["stepfun", "stepfun-ai"],
        offerings: &[
            ProviderOffering::chat("https://api.stepfun.com/v1"),
            ProviderOffering::responses("https://api.stepfun.com/v1"),
            // Anthropic 兼容的 base 就是裸 host（官方完整路径 /v1/messages）
            ProviderOffering::anthropic("https://api.stepfun.com"),
        ],
    },
    ProviderPreset {
        id: "sensenova",
        name_key: "provider.sensenova",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://console.sensecore.cn/iam/apikey"),
        doc_url: Some("https://www.sensecore.cn/help/docs"),
        badge: "商",
        catalog_provider_ids: &["sensenova"],
        offerings: &[ProviderOffering::chat(
            "https://api.sensenova.cn/compatible-mode/v2",
        )],
    },
    ProviderPreset {
        id: "zai",
        name_key: "provider.zai",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://z.ai/manage-apikey/apikey-list"),
        doc_url: Some("https://docs.z.ai"),
        badge: "Z",
        catalog_provider_ids: &["zai"],
        offerings: &[
            ProviderOffering::chat("https://api.z.ai/api/paas/v4"),
            // 官方 Claude Code 文档给出的 base：https://api.z.ai/api/anthropic
            ProviderOffering::anthropic("https://api.z.ai/api/anthropic"),
        ],
    },
    ProviderPreset {
        id: "qianfan",
        name_key: "provider.qianfan",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://console.bce.baidu.com/iam/#/iam/apikey/list"),
        doc_url: Some("https://cloud.baidu.com/doc/WENXINWORKSHOP/index.html"),
        badge: "百",
        // 上游目录暂无 baidu 条目：界面会显示"目录无该厂商数据，请刷新模型列表"
        catalog_provider_ids: &["baidu", "qianfan"],
        offerings: &[
            ProviderOffering::chat("https://qianfan.baidubce.com/v2"),
            ProviderOffering::responses("https://qianfan.baidubce.com/v2"),
            ProviderOffering::anthropic("https://qianfan.baidubce.com/anthropic"),
        ],
    },
    ProviderPreset {
        id: "hunyuan",
        name_key: "provider.hunyuan",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://console.cloud.tencent.com/hunyuan/api-key"),
        doc_url: Some("https://cloud.tencent.com/document/product/1729"),
        badge: "腾",
        catalog_provider_ids: &["tencent", "hunyuan"],
        offerings: &[
            ProviderOffering::chat("https://api.hunyuan.cloud.tencent.com/v1"),
            ProviderOffering::anthropic("https://api.hunyuan.cloud.tencent.com/anthropic"),
        ],
    },
    // ==================== 海外官方 ====================
    ProviderPreset {
        id: "openai",
        name_key: "provider.openai",
        category: ProviderCategory::Official,
        keyless: false,
        key_url: Some("https://platform.openai.com/api-keys"),
        doc_url: Some("https://platform.openai.com/docs"),
        badge: "O",
        catalog_provider_ids: &["openai"],
        offerings: &[
            ProviderOffering::chat("https://api.openai.com/v1"),
            ProviderOffering::responses("https://api.openai.com/v1"),
        ],
    },
    ProviderPreset {
        id: "anthropic",
        name_key: "provider.anthropic",
        category: ProviderCategory::Official,
        keyless: false,
        key_url: Some("https://console.anthropic.com/settings/keys"),
        doc_url: Some("https://docs.anthropic.com"),
        badge: "A",
        catalog_provider_ids: &["anthropic"],
        offerings: &[ProviderOffering::anthropic("https://api.anthropic.com")],
    },
    ProviderPreset {
        id: "google",
        name_key: "provider.google",
        category: ProviderCategory::Official,
        keyless: false,
        key_url: Some("https://aistudio.google.com/app/apikey"),
        doc_url: Some("https://ai.google.dev/gemini-api/docs"),
        badge: "G",
        catalog_provider_ids: &["google"],
        offerings: &[ProviderOffering::gemini(
            "https://generativelanguage.googleapis.com",
        )],
    },
    ProviderPreset {
        id: "xai",
        name_key: "provider.xai",
        category: ProviderCategory::Official,
        keyless: false,
        key_url: Some("https://console.x.ai"),
        doc_url: Some("https://docs.x.ai"),
        badge: "X",
        catalog_provider_ids: &["xai"],
        offerings: &[ProviderOffering::chat("https://api.x.ai/v1")],
    },
    ProviderPreset {
        id: "mistral",
        name_key: "provider.mistral",
        category: ProviderCategory::Official,
        keyless: false,
        key_url: Some("https://console.mistral.ai/api-keys"),
        doc_url: Some("https://docs.mistral.ai"),
        badge: "M",
        catalog_provider_ids: &["mistral"],
        offerings: &[ProviderOffering::chat("https://api.mistral.ai/v1")],
    },
    ProviderPreset {
        id: "groq",
        name_key: "provider.groq",
        category: ProviderCategory::Official,
        keyless: false,
        key_url: Some("https://console.groq.com/keys"),
        doc_url: Some("https://console.groq.com/docs"),
        badge: "Q",
        catalog_provider_ids: &["groq"],
        offerings: &[ProviderOffering::chat("https://api.groq.com/openai/v1")],
    },
    ProviderPreset {
        id: "perplexity",
        name_key: "provider.perplexity",
        category: ProviderCategory::Official,
        keyless: false,
        key_url: Some("https://www.perplexity.ai/settings/api"),
        doc_url: Some("https://docs.perplexity.ai"),
        badge: "P",
        catalog_provider_ids: &["perplexity"],
        offerings: &[ProviderOffering::chat("https://api.perplexity.ai")],
    },
    // ==================== 聚合与中转 ====================
    ProviderPreset {
        id: "siliconflow",
        name_key: "provider.siliconflow",
        category: ProviderCategory::Gateway,
        keyless: false,
        key_url: Some("https://cloud.siliconflow.cn/account/ak"),
        doc_url: Some("https://docs.siliconflow.cn"),
        badge: "硅",
        catalog_provider_ids: &["siliconflow", "siliconflow-cn"],
        offerings: &[ProviderOffering::chat("https://api.siliconflow.cn/v1")],
    },
    ProviderPreset {
        id: "modelscope",
        name_key: "provider.modelscope",
        category: ProviderCategory::Gateway,
        keyless: false,
        key_url: Some("https://modelscope.cn/my/myaccesstoken"),
        doc_url: Some("https://www.modelscope.cn/docs"),
        badge: "魔",
        catalog_provider_ids: &["modelscope"],
        offerings: &[ProviderOffering::chat(
            "https://api-inference.modelscope.cn/v1",
        )],
    },
    ProviderPreset {
        id: "openrouter",
        name_key: "provider.openrouter",
        category: ProviderCategory::Gateway,
        keyless: false,
        key_url: Some("https://openrouter.ai/keys"),
        doc_url: Some("https://openrouter.ai/docs"),
        badge: "OR",
        catalog_provider_ids: &["openrouter"],
        offerings: &[ProviderOffering::chat("https://openrouter.ai/api/v1")],
    },
    ProviderPreset {
        id: "together",
        name_key: "provider.together",
        category: ProviderCategory::Gateway,
        keyless: false,
        key_url: Some("https://api.together.xyz/settings/api-keys"),
        doc_url: Some("https://docs.together.ai"),
        badge: "T",
        catalog_provider_ids: &["togetherai"],
        offerings: &[ProviderOffering::chat("https://api.together.xyz/v1")],
    },
    ProviderPreset {
        id: "fireworks",
        name_key: "provider.fireworks",
        category: ProviderCategory::Gateway,
        keyless: false,
        key_url: Some("https://fireworks.ai/account/api-keys"),
        doc_url: Some("https://docs.fireworks.ai"),
        badge: "F",
        catalog_provider_ids: &["fireworks", "fireworks-ai"],
        offerings: &[ProviderOffering::chat(
            "https://api.fireworks.ai/inference/v1",
        )],
    },
    ProviderPreset {
        id: "nvidia",
        name_key: "provider.nvidia",
        category: ProviderCategory::Gateway,
        keyless: false,
        key_url: Some("https://build.nvidia.com/settings/api-keys"),
        doc_url: Some("https://docs.nvidia.com/nim"),
        badge: "N",
        catalog_provider_ids: &["nvidia"],
        offerings: &[ProviderOffering::chat(
            "https://integrate.api.nvidia.com/v1",
        )],
    },
    ProviderPreset {
        id: "cerebras",
        name_key: "provider.cerebras",
        category: ProviderCategory::Gateway,
        keyless: false,
        key_url: Some("https://cloud.cerebras.ai"),
        doc_url: Some("https://inference-docs.cerebras.ai"),
        badge: "C",
        catalog_provider_ids: &["cerebras"],
        offerings: &[ProviderOffering::chat("https://api.cerebras.ai/v1")],
    },
    // ==================== 本地部署 ====================
    ProviderPreset {
        id: "ollama",
        name_key: "provider.ollama",
        category: ProviderCategory::Local,
        keyless: true,
        key_url: None,
        doc_url: Some("https://docs.ollama.com/api/openai-compatibility"),
        badge: "OL",
        catalog_provider_ids: &[],
        offerings: &[ProviderOffering::chat("http://localhost:11434/v1")],
    },
    ProviderPreset {
        id: "lmstudio",
        name_key: "provider.lmstudio",
        category: ProviderCategory::Local,
        keyless: true,
        key_url: None,
        doc_url: Some("https://lmstudio.ai/docs/app/api"),
        badge: "LM",
        catalog_provider_ids: &[],
        offerings: &[ProviderOffering::chat("http://localhost:1234/v1")],
    },
    ProviderPreset {
        id: "vllm",
        name_key: "provider.vllm",
        category: ProviderCategory::Local,
        keyless: true,
        key_url: None,
        doc_url: Some("https://docs.vllm.ai/en/latest/serving/openai_compatible_server.html"),
        badge: "vL",
        catalog_provider_ids: &[],
        offerings: &[ProviderOffering::chat("http://localhost:8000/v1")],
    },
    // ==================== 自定义 ====================
    ProviderPreset {
        id: "custom",
        name_key: "provider.custom",
        category: ProviderCategory::Custom,
        keyless: false,
        key_url: None,
        doc_url: None,
        badge: "+",
        catalog_provider_ids: &[],
        offerings: &[
            ProviderOffering::chat(""),
            ProviderOffering::responses(""),
            ProviderOffering::anthropic(""),
            ProviderOffering::gemini(""),
        ],
    },
];

/// 按 id 查预置（规范化精确匹配）。
pub fn preset_by_id(id: &str) -> Option<&'static ProviderPreset> {
    let needle = id.trim().to_lowercase();
    BUILTIN_PRESETS
        .iter()
        .find(|p| p.id.to_lowercase() == needle)
}

/// 分组顺序：**国内厂商第一**。
pub const CATEGORY_ORDER: [ProviderCategory; 5] = [
    ProviderCategory::China,
    ProviderCategory::Official,
    ProviderCategory::Gateway,
    ProviderCategory::Local,
    ProviderCategory::Custom,
];

/// 按分类分组（UI 分组渲染用），顺序由 `CATEGORY_ORDER` 决定。
pub fn presets_by_category() -> Vec<(ProviderCategory, Vec<&'static ProviderPreset>)> {
    CATEGORY_ORDER
        .into_iter()
        .map(|category| {
            let items: Vec<&'static ProviderPreset> = BUILTIN_PRESETS
                .iter()
                .filter(|p| p.category == category)
                .collect();
            (category, items)
        })
        .filter(|(_, items)| !items.is_empty())
        .collect()
}

/// 搜索过滤（匹配 id / badge 的朴素版本；带本地化名称的见
/// `search_presets_localized`）。
pub fn search_presets(query: &str) -> Vec<&'static ProviderPreset> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return BUILTIN_PRESETS.iter().collect();
    }
    BUILTIN_PRESETS
        .iter()
        .filter(|p| {
            p.id.contains(&needle)
                || p.name_key.to_lowercase().contains(&needle)
                || p.badge.to_lowercase().contains(&needle)
        })
        .collect()
}

/// 带本地化名称的搜索（宿主传入 `name_of(preset)`）。
pub fn search_presets_localized<F>(query: &str, name_of: F) -> Vec<&'static ProviderPreset>
where
    F: Fn(&ProviderPreset) -> String,
{
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return BUILTIN_PRESETS.iter().collect();
    }
    BUILTIN_PRESETS
        .iter()
        .filter(|p| {
            p.id.contains(&needle)
                || p.badge.to_lowercase().contains(&needle)
                || name_of(p).to_lowercase().contains(&needle)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_lowercase() {
        let mut ids: Vec<&str> = BUILTIN_PRESETS.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count);
        for id in ids {
            assert_eq!(id, id.to_lowercase());
        }
    }

    #[test]
    fn china_comes_first_and_deepseek_is_domestic() {
        let groups = presets_by_category();
        assert_eq!(groups[0].0, ProviderCategory::China);
        // DeepSeek 是国内厂商，不该和海外官方混在一起
        let deepseek = preset_by_id("deepseek").unwrap();
        assert_eq!(deepseek.category, ProviderCategory::China);
        let china_ids: Vec<&str> = groups[0].1.iter().map(|p| p.id).collect();
        for expected in ["deepseek", "zhipu", "dashscope", "moonshot", "ark"] {
            assert!(china_ids.contains(&expected), "国内组缺少 {expected}");
        }
    }

    #[test]
    fn presets_do_not_hardcode_model_names() {
        // 关键回归：预置里不得再出现写死的模型名。
        // 模型清单只能来自 ① /models 实时拉取 ② 目录按厂商匹配 ③ 用户手填。
        for preset in BUILTIN_PRESETS {
            for offering in preset.offerings {
                let url = offering.default_endpoint;
                let has_model_like = ["gpt-", "claude-", "deepseek-", "glm-", "qwen-", "moonshot-"]
                    .iter()
                    .any(|needle| url.contains(needle));
                assert!(
                    !has_model_like,
                    "{} 的端点里不该出现模型名: {url}",
                    preset.id
                );
            }
        }
    }

    #[test]
    fn cloud_presets_declare_catalog_provider_ids() {
        for preset in BUILTIN_PRESETS {
            match preset.category {
                ProviderCategory::Local | ProviderCategory::Custom => {
                    assert!(!preset.has_catalog_source(), "{} 不该有目录来源", preset.id);
                }
                _ => assert!(
                    preset.has_catalog_source(),
                    "{} 声明了厂商却没有目录匹配键，推荐模型会永远为空",
                    preset.id
                ),
            }
        }
    }

    #[test]
    fn catalog_provider_ids_are_unique_across_presets() {
        // 同一个上游 provider 不该被两个预置同时认领（否则推荐模型会串台）
        let mut seen: Vec<&str> = Vec::new();
        for preset in BUILTIN_PRESETS {
            for id in preset.catalog_provider_ids {
                assert!(!seen.contains(id), "上游 provider 键 {id} 被多个预置共用");
                seen.push(id);
            }
        }
    }

    #[test]
    fn every_preset_has_a_usable_default_offering() {
        for preset in BUILTIN_PRESETS {
            assert!(!preset.offerings.is_empty(), "{} 没有 offering", preset.id);
            if preset.category != ProviderCategory::Custom {
                assert!(
                    !preset.offerings[0].default_endpoint.is_empty(),
                    "{} 首个协议必须有默认地址",
                    preset.id
                );
            }
        }
    }

    /// 一个厂商的 offerings 里**同一协议只能出现一次**：协议下拉与
    /// "切换协议自动带出端点"都按协议查找，重复会导致带出哪个端点不确定。
    #[test]
    fn a_preset_never_declares_the_same_protocol_twice() {
        for preset in BUILTIN_PRESETS {
            let mut seen: Vec<ProtocolKind> = Vec::new();
            for offering in preset.offerings {
                assert!(
                    !seen.contains(&offering.protocol),
                    "{} 重复声明了协议 {:?}",
                    preset.id,
                    offering.protocol
                );
                seen.push(offering.protocol);
            }
        }
    }

    /// 端点用于拼接动作路径（如 base + `/chat/completions`），
    /// 统一不带结尾斜杠，避免出现 `//chat/completions` 这类地址。
    #[test]
    fn preset_endpoints_have_no_trailing_slash() {
        for preset in BUILTIN_PRESETS {
            for offering in preset.offerings {
                assert!(
                    !offering.default_endpoint.ends_with('/'),
                    "{} 的端点不应带结尾斜杠: {}",
                    preset.id,
                    offering.default_endpoint
                );
            }
        }
    }

    #[test]
    fn official_endpoints_use_https_and_locals_use_http() {
        for preset in BUILTIN_PRESETS {
            for offering in preset.offerings {
                let url = offering.default_endpoint;
                if url.is_empty() {
                    continue;
                }
                match preset.category {
                    ProviderCategory::Local => assert!(
                        url.starts_with("http://localhost"),
                        "{} 本地端点异常: {url}",
                        preset.id
                    ),
                    _ => assert!(
                        url.starts_with("https://"),
                        "{} 应使用 https: {url}",
                        preset.id
                    ),
                }
            }
        }
    }

    #[test]
    fn local_presets_are_keyless() {
        for preset in BUILTIN_PRESETS
            .iter()
            .filter(|p| p.category == ProviderCategory::Local)
        {
            assert!(preset.keyless);
            assert!(preset.key_url.is_none());
        }
    }

    #[test]
    fn cloud_presets_provide_key_url() {
        for preset in BUILTIN_PRESETS.iter().filter(|p| {
            !matches!(
                p.category,
                ProviderCategory::Local | ProviderCategory::Custom
            )
        }) {
            assert!(preset.key_url.is_some(), "{} 缺少申 Key 链接", preset.id);
        }
    }

    #[test]
    fn deepseek_offers_both_protocols_with_distinct_endpoints() {
        let deepseek = preset_by_id("deepseek").unwrap();
        let chat = deepseek.offering(ProtocolKind::OpenAiChat).unwrap();
        let anthropic = deepseek.offering(ProtocolKind::AnthropicMessages).unwrap();
        assert_ne!(chat.default_endpoint, anthropic.default_endpoint);
        assert_eq!(chat.default_endpoint, "https://api.deepseek.com");
        assert_eq!(
            anthropic.default_endpoint,
            "https://api.deepseek.com/anthropic"
        );
    }

    #[test]
    fn search_matches_id_badge_and_localized_name() {
        assert!(search_presets("deep").iter().any(|p| p.id == "deepseek"));
        assert!(search_presets("or").iter().any(|p| p.id == "openrouter"));
        assert!(search_presets("").len() == BUILTIN_PRESETS.len());
        let found = search_presets_localized("智谱", |p| {
            if p.id == "zhipu" {
                "智谱 GLM".to_string()
            } else {
                p.id.to_string()
            }
        });
        assert!(found.iter().any(|p| p.id == "zhipu"));
    }

    #[test]
    fn custom_preset_covers_all_protocols_with_blank_endpoint() {
        let custom = preset_by_id("custom").unwrap();
        assert_eq!(custom.offerings.len(), 4);
        for offering in custom.offerings {
            assert!(offering.default_endpoint.is_empty());
        }
    }
}
