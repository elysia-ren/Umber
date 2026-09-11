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
    /// **默认计费方式（按量）**的协议端点。
    pub offerings: &'static [ProviderOffering],
    /// 订阅制计费方式（Coding Plan / Token Plan 等）。
    ///
    /// 为什么单独一层、而不是在厂商列表里再复制一家「智谱 · 编码套餐」：
    /// - 这些端点是**订阅专属**，与按量端点**不可混用**（官方明确警告，混用会失败
    ///   或产生额外费用）；
    /// - 同一家厂商的订阅套餐与按量套餐，支持的协议 / 模型往往不同；
    /// - 放在厂商内的「计费方式」里切换，用户不会在列表里选错同名的第二家。
    ///
    /// 空数组 = 该厂商只有按量一种计费方式。
    pub plans: &'static [ProviderPlan],
}

/// 一种**订阅制**计费方式及其端点。
///
/// 默认（按量）计费方式不是 ProviderPlan——它就是 ProviderPreset::offerings，
/// 索引 0 永远留给它，这样既有代码路径无需区分「有没有套餐」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderPlan {
    pub id: &'static str,
    /// 计费方式名称的 i18n key。
    pub name_key: &'static str,
    /// 订阅制：界面需要提示「需订阅」，并且**不会**在选中厂商时被自动选中。
    pub subscription: bool,
    pub offerings: &'static [ProviderOffering],
}

/// 计费方式索引 0（按量）的名称 key；所有厂商共用。
pub const PLAN_PAYG_NAME_KEY: &str = "plan.payg";

impl ProviderPreset {
    pub fn default_protocol(&self) -> ProtocolKind {
        self.offerings[0].protocol
    }

    pub fn offering(&self, protocol: ProtocolKind) -> Option<&ProviderOffering> {
        self.offerings.iter().find(|o| o.protocol == protocol)
    }

    /// 全部计费方式数量（含默认的按量）。
    pub fn plan_count(&self) -> usize {
        1 + self.plans.len()
    }

    /// 指定计费方式的端点集合。索引 0 = 按量；越界退回按量。
    pub fn plan_offerings(&self, plan: usize) -> &'static [ProviderOffering] {
        if plan == 0 {
            self.offerings
        } else {
            self.plans
                .get(plan - 1)
                .map(|p| p.offerings)
                .unwrap_or(self.offerings)
        }
    }

    /// 指定计费方式下某协议的端点。
    pub fn offering_in(&self, plan: usize, protocol: ProtocolKind) -> Option<&ProviderOffering> {
        self.plan_offerings(plan)
            .iter()
            .find(|o| o.protocol == protocol)
    }

    /// 指定计费方式的名称 key。
    pub fn plan_name_key(&self, plan: usize) -> &'static str {
        match plan {
            0 => PLAN_PAYG_NAME_KEY,
            i => self
                .plans
                .get(i - 1)
                .map(|p| p.name_key)
                .unwrap_or(PLAN_PAYG_NAME_KEY),
        }
    }

    /// 指定计费方式是否为订阅制。
    pub fn plan_is_subscription(&self, plan: usize) -> bool {
        plan > 0
            && self
                .plans
                .get(plan - 1)
                .map(|p| p.subscription)
                .unwrap_or(false)
    }

    pub fn subtitle_key(&self) -> &'static str {
        self.category.title_key()
    }

    /// 该厂商是否有可查的目录数据来源（本地部署与自定义没有）。
    pub fn has_catalog_source(&self) -> bool {
        !self.catalog_provider_ids.is_empty()
    }
}

// ==================== 订阅制套餐端点 ====================
//
// 这些端点**只有订阅对应套餐才能用**，且与按量端点**不可混用**（官方明确警告，
// 混用会失败或产生额外费用）。因此它们不混进厂商的默认 offerings，而是作为
// 厂商设置里的「计费方式」由用户显式切换。

/// 智谱 GLM 编码套餐（Coding Plan）。
const ZHIPU_CODING: &[ProviderPlan] = &[ProviderPlan {
    id: "coding",
    name_key: "plan.zhipu.coding",
    subscription: true,
    offerings: &[
        ProviderOffering::chat("https://open.bigmodel.cn/api/coding/paas/v4"),
        ProviderOffering::responses("https://open.bigmodel.cn/api/v1"),
        // Anthropic 面与按量同址（官方 Coding Plan 文档也指向该 base）
        ProviderOffering::anthropic("https://open.bigmodel.cn/api/anthropic"),
    ],
}];

/// 智谱国际站（Z.ai）GLM Coding Plan。
const ZAI_CODING: &[ProviderPlan] = &[ProviderPlan {
    id: "coding",
    name_key: "plan.zai.coding",
    subscription: true,
    offerings: &[
        ProviderOffering::chat("https://api.z.ai/api/coding/paas/v4"),
        ProviderOffering::responses("https://api.z.ai/api/v1"),
        ProviderOffering::anthropic("https://api.z.ai/api/anthropic"),
    ],
}];

/// 火山方舟 Coding Plan。官方特别提示：该套餐下不能用 /api/v3（会产生额外费用）。
const ARK_CODING: &[ProviderPlan] = &[ProviderPlan {
    id: "coding",
    name_key: "plan.ark.coding",
    subscription: true,
    offerings: &[
        ProviderOffering::chat("https://ark.cn-beijing.volces.com/api/coding/v3"),
        ProviderOffering::anthropic("https://ark.cn-beijing.volces.com/api/coding"),
    ],
}];

/// 阶跃星辰 Step Plan。
const STEPFUN_STEP_PLAN: &[ProviderPlan] = &[ProviderPlan {
    id: "step_plan",
    name_key: "plan.stepfun.plan",
    subscription: true,
    offerings: &[
        ProviderOffering::chat("https://api.stepfun.com/step_plan/v1"),
        ProviderOffering::anthropic("https://api.stepfun.com/step_plan"),
    ],
}];

/// Kimi Code 会员额度：只提供 Anthropic 面，与开放平台 API Key 不通用。
const MOONSHOT_KIMI_CODE: &[ProviderPlan] = &[ProviderPlan {
    id: "kimi_code",
    name_key: "plan.moonshot.kimi_code",
    subscription: true,
    offerings: &[ProviderOffering::anthropic("https://api.kimi.com/coding")],
}];

/// 阿里云百炼的两种订阅制计费方式（Coding Plan / Token Plan）。
///
/// 官方《Token Plan 快速开始》明确写着：Token Plan、Coding Plan 与按量付费的
/// API Key 与 Base URL **完全隔离，必须配套使用，不可混用**——这正是「计费方式」
/// 要建模的东西，混用只会得到认证失败。该页同时给出了 Coding Plan 的 OpenAI
/// 面（此前只有 Anthropic 面）与 Token Plan 的两个面。
/// 国际站（coding-intl / 新加坡等）不在本预置内，用「自定义」接。
const DASHSCOPE_PLANS: &[ProviderPlan] = &[
    ProviderPlan {
        id: "coding",
        name_key: "plan.dashscope.coding",
        subscription: true,
        offerings: &[
            ProviderOffering::chat("https://coding.dashscope.aliyuncs.com/v1"),
            ProviderOffering::anthropic("https://coding.dashscope.aliyuncs.com/apps/anthropic"),
        ],
    },
    ProviderPlan {
        id: "token_plan",
        name_key: "plan.dashscope.token_plan",
        subscription: true,
        offerings: &[
            ProviderOffering::chat(
                "https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1",
            ),
            ProviderOffering::anthropic(
                "https://token-plan.cn-beijing.maas.aliyuncs.com/apps/anthropic",
            ),
        ],
    },
];
/// 小米 MiMo Token Plan（订阅）。
///
/// 官方《Quick Access》给出三个集群，端点各不相同，且订阅页才显示账号属于哪个：
/// 只挑一个做默认，另外两个集群的用户一开箱就是"连接失败"——所以三个都列出来，
/// 让用户按自己控制台上显示的集群选（集群选错等同于地址错）。
const MIMO_TOKEN_PLAN: &[ProviderPlan] = &[
    ProviderPlan {
        id: "token_plan_cn",
        name_key: "plan.mimo.token_plan_cn",
        subscription: true,
        offerings: &[
            ProviderOffering::chat("https://token-plan-cn.xiaomimimo.com/v1"),
            ProviderOffering::responses("https://token-plan-cn.xiaomimimo.com/v1"),
            ProviderOffering::anthropic("https://token-plan-cn.xiaomimimo.com/anthropic"),
        ],
    },
    ProviderPlan {
        id: "token_plan_sgp",
        name_key: "plan.mimo.token_plan_sgp",
        subscription: true,
        offerings: &[
            ProviderOffering::chat("https://token-plan-sgp.xiaomimimo.com/v1"),
            ProviderOffering::responses("https://token-plan-sgp.xiaomimimo.com/v1"),
            ProviderOffering::anthropic("https://token-plan-sgp.xiaomimimo.com/anthropic"),
        ],
    },
    ProviderPlan {
        id: "token_plan_ams",
        name_key: "plan.mimo.token_plan_ams",
        subscription: true,
        offerings: &[
            ProviderOffering::chat("https://token-plan-ams.xiaomimimo.com/v1"),
            ProviderOffering::responses("https://token-plan-ams.xiaomimimo.com/v1"),
            ProviderOffering::anthropic("https://token-plan-ams.xiaomimimo.com/anthropic"),
        ],
    },
];

/// 腾讯云 TokenHub 的两种订阅制计费方式。
///
/// TokenHub 与「腾讯混元大模型」（api.hunyuan.cloud.tencent.com）是**两个产品**：
/// 混元是模型本身，TokenHub 是腾讯云的大模型服务平台（同一个平台上还卖 DeepSeek、
/// GLM 等家外模型），所以它是独立预置而不是混元的套餐。
///
/// 官方 Coding Plan 文档逐字写着：订阅专属的 API Key 和 Base URL 与腾讯云预付费/
/// 后付费的 API Key（sk-xxxx）和 Base URL「不互通，请勿混用」——正是「计费方式」。
/// Token Plan 另有 `tokenhub.tencentmaas.com/plan/…` 的写法（官方的工具接入页），
/// 本预置采用套餐总览页给出的 `api.lkeap.cloud.tencent.com/plan/…`。
const TOKENHUB_PLANS: &[ProviderPlan] = &[
    ProviderPlan {
        id: "coding",
        name_key: "plan.tokenhub.coding",
        subscription: true,
        offerings: &[
            ProviderOffering::chat("https://api.lkeap.cloud.tencent.com/coding/v3"),
            ProviderOffering::anthropic("https://api.lkeap.cloud.tencent.com/coding/anthropic"),
        ],
    },
    ProviderPlan {
        id: "token_plan",
        name_key: "plan.tokenhub.token_plan",
        subscription: true,
        offerings: &[
            ProviderOffering::chat("https://api.lkeap.cloud.tencent.com/plan/v3"),
            ProviderOffering::anthropic("https://api.lkeap.cloud.tencent.com/plan/anthropic"),
        ],
    },
];

/// 讯飞星火 Coding Plan（官方名为 Astron Coding Plan）。
///
/// 讯飞的**按量**端点只提供 OpenAI 兼容；Anthropic 面只在 Coding Plan 下给出，
/// 所以这里是一条套餐而不是默认 offering。
const IFLYTEK_CODING: &[ProviderPlan] = &[ProviderPlan {
    id: "coding",
    name_key: "plan.iflytek.coding",
    subscription: true,
    offerings: &[
        ProviderOffering::chat("https://maas-coding-api.cn-huabei-1.xf-yun.com/v2"),
        ProviderOffering::anthropic("https://maas-coding-api.cn-huabei-1.xf-yun.com/anthropic"),
    ],
}];

/// 华为云盘古 Token Plan。
const HUAWEI_TOKEN_PLAN: &[ProviderPlan] = &[ProviderPlan {
    id: "token_plan",
    name_key: "plan.huawei.token_plan",
    subscription: true,
    offerings: &[
        ProviderOffering::chat("https://api.modelarts-maas.com/plan/v2"),
        ProviderOffering::anthropic("https://api.modelarts-maas.com/plan/anthropic"),
    ],
}];

/// 京东云言犀 TokenPlan（官方已把 CodingPlan 停止并引导到 TokenPlan）。
const JDCLOUD_TOKEN_PLAN: &[ProviderPlan] = &[ProviderPlan {
    id: "token_plan",
    name_key: "plan.jdcloud.token_plan",
    subscription: true,
    offerings: &[
        ProviderOffering::chat("https://modelservice.jdcloud.com/tokenPlan/openai/v1"),
        ProviderOffering::anthropic("https://modelservice.jdcloud.com/tokenPlan/anthropic"),
    ],
}];

/// 天翼云星辰编程 Token Plan。官方只给出 OpenAI 面，Anthropic 面未记录 → 只声明已确认的。
const TELECOM_CODING: &[ProviderPlan] = &[ProviderPlan {
    id: "coding",
    name_key: "plan.telecom.coding",
    subscription: true,
    offerings: &[ProviderOffering::chat("https://ai.ctaigw.cn/coding/v1")],
}];

/// 百度千帆 Token Plan（个人版 / 企业版）。
///
/// 官方已停售 Coding Plan（2026-07-13 起停止新购，存量可用到周期结束），
/// 并引导迁移到 Token Plan，所以这里只建 Token Plan 的两种形态。
/// 端点逐字来自官方 Token Plan 总览页的「兼容 OpenAI / Anthropic 接口协议的
/// Base URL」表；两种形态的 base URL 不同，因此是两条计费方式。
const QIANFAN_TOKEN_PLAN: &[ProviderPlan] = &[
    ProviderPlan {
        id: "token_plan_personal",
        name_key: "plan.qianfan.token_plan_personal",
        subscription: true,
        offerings: &[
            ProviderOffering::chat("https://qianfan.baidubce.com/v2/tokenplan/personal"),
            ProviderOffering::anthropic(
                "https://qianfan.baidubce.com/anthropic/tokenplan/personal",
            ),
        ],
    },
    ProviderPlan {
        id: "token_plan_team",
        name_key: "plan.qianfan.token_plan_team",
        subscription: true,
        offerings: &[
            ProviderOffering::chat("https://qianfan.baidubce.com/v2/tokenplan/team"),
            ProviderOffering::anthropic("https://qianfan.baidubce.com/anthropic/tokenplan/team"),
        ],
    },
];

/// 上游目录（models.dev / LiteLLM）尚未收录的厂商。
///
/// 这些厂商的「推荐模型」会是空的：界面会如实引导用户点「刷新模型列表」或手动
/// 填 Model ID，而不是显示不存在的型号（§X.16）。它们依然值得存在——预置的价值
/// 首先是**端点与协议的正确答案**，那部分逐条核过官方文档。
/// 一旦上游收录，就把 id 从这份名单挪进该预置的 `catalog_provider_ids`。
pub const PRESETS_WITHOUT_CATALOG: &[&str] = &[
    "iflytek", "internlm", "huawei", "jdcloud", "antling", "telecom",
];

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
        plans: &[],
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
        plans: ZHIPU_CODING,
    },
    ProviderPreset {
        id: "dashscope",
        name_key: "provider.dashscope",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://bailian.console.aliyun.com/?apiKey=1"),
        doc_url: Some("https://help.aliyun.com/zh/model-studio/"),
        badge: "阿",
        catalog_provider_ids: &[
            "alibaba",
            "alibaba-cn",
            "alibaba-coding-plan-cn",
            "alibaba-token-plan-cn",
        ],
        offerings: &[
            ProviderOffering::chat("https://dashscope.aliyuncs.com/compatible-mode/v1"),
            // 官方 Claude Code 文档给出的 Anthropic 兼容端点
            ProviderOffering::anthropic("https://dashscope.aliyuncs.com/apps/anthropic"),
        ],
        plans: DASHSCOPE_PLANS,
    },
    ProviderPreset {
        id: "moonshot",
        name_key: "provider.moonshot",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://platform.moonshot.cn/console/api-keys"),
        doc_url: Some("https://platform.moonshot.cn/docs"),
        badge: "K",
        catalog_provider_ids: &["moonshotai", "moonshotai-cn", "kimi-for-coding"],
        offerings: &[
            ProviderOffering::chat("https://api.moonshot.cn/v1"),
            ProviderOffering::responses("https://api.moonshot.cn/v1"),
            // 官方文档：Anthropic 兼容 base 为 https://api.moonshot.cn/anthropic
            ProviderOffering::anthropic("https://api.moonshot.cn/anthropic"),
        ],
        plans: MOONSHOT_KIMI_CODE,
    },
    ProviderPreset {
        id: "ark",
        name_key: "provider.ark",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey"),
        doc_url: Some("https://www.volcengine.com/docs/82379"),
        badge: "火",
        catalog_provider_ids: &["volcengine", "volcengine-coding-plan"],
        offerings: &[
            ProviderOffering::chat("https://ark.cn-beijing.volces.com/api/v3"),
            ProviderOffering::responses("https://ark.cn-beijing.volces.com/api/v3"),
            // 官方 Messages（Anthropic 兼容）端点，非 Coding Plan 专属
            ProviderOffering::anthropic("https://ark.cn-beijing.volces.com/api/compatible"),
        ],
        plans: ARK_CODING,
    },
    ProviderPreset {
        id: "minimax",
        name_key: "provider.minimax",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://platform.minimaxi.com/user-center/basic-information/interface-key"),
        doc_url: Some("https://platform.minimaxi.com/document"),
        badge: "mm",
        catalog_provider_ids: &[
            "minimax",
            "minimax-cn",
            "minimax-coding-plan",
            "minimax-cn-coding-plan",
        ],
        // MiniMax 的 Token Plan（原 Coding Plan）**没有独立端点**：官方文档里订阅与
        // 按量用的是同一个 base URL，区分物是 Subscription Key。因此这里不设「计费
        // 方式」，用户订阅后直接把 Key 换成订阅 Key 即可。
        offerings: &[
            // 国内站：api.minimax.chat 已过期，官方文档现行域名为 api.minimax.cn
            // （国际站为 api.minimax.io，见 docs 的国内/国际两套）
            ProviderOffering::chat("https://api.minimax.cn/v1"),
            ProviderOffering::responses("https://api.minimax.cn/v1"),
            ProviderOffering::anthropic("https://api.minimax.cn/anthropic"),
        ],
        plans: &[],
    },
    ProviderPreset {
        id: "stepfun",
        name_key: "provider.stepfun",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://platform.stepfun.com/interface-key"),
        doc_url: Some("https://platform.stepfun.com/docs"),
        badge: "阶",
        catalog_provider_ids: &[
            "stepfun",
            "stepfun-ai",
            "stepfun-step-plan",
            "stepfun-ai-step-plan",
        ],
        offerings: &[
            ProviderOffering::chat("https://api.stepfun.com/v1"),
            ProviderOffering::responses("https://api.stepfun.com/v1"),
            // Anthropic 兼容的 base 就是裸 host（官方完整路径 /v1/messages）
            ProviderOffering::anthropic("https://api.stepfun.com"),
        ],
        plans: STEPFUN_STEP_PLAN,
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
        plans: &[],
    },
    ProviderPreset {
        id: "zai",
        name_key: "provider.zai",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://z.ai/manage-apikey/apikey-list"),
        doc_url: Some("https://docs.z.ai"),
        badge: "Z",
        catalog_provider_ids: &["zai", "zai-coding-plan"],
        offerings: &[
            ProviderOffering::chat("https://api.z.ai/api/paas/v4"),
            // 官方 Claude Code 文档给出的 base：https://api.z.ai/api/anthropic
            ProviderOffering::anthropic("https://api.z.ai/api/anthropic"),
        ],
        plans: ZAI_CODING,
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
            // 官方 Claude Code 文档逐字给出 ANTHROPIC_BASE_URL，并注明用「千帆通用
            // API Key」——因此它归在按量付费名下（该页不在「按量付费」标题下，属推断）。
            ProviderOffering::anthropic("https://qianfan.baidubce.com/anthropic"),
        ],
        plans: QIANFAN_TOKEN_PLAN,
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
        plans: &[],
    },
    ProviderPreset {
        id: "xiaomi",
        name_key: "provider.xiaomi",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://platform.xiaomimimo.com/#/console/api-keys"),
        doc_url: Some("https://mimo.mi.com"),
        badge: "米",
        // 上游目录按计费方式拆成四个 provider 键：按量 + 三个 Token Plan 集群
        catalog_provider_ids: &[
            "xiaomi",
            "xiaomi-token-plan-cn",
            "xiaomi-token-plan-sgp",
            "xiaomi-token-plan-ams",
        ],
        offerings: &[
            // 官方《First API Call》：OpenAI 兼容 https://api.xiaomimimo.com/v1
            ProviderOffering::chat("https://api.xiaomimimo.com/v1"),
            // 官方《OpenAI Responses API Compatibility》：POST /v1/responses
            ProviderOffering::responses("https://api.xiaomimimo.com/v1"),
            // 官方《Anthropic Messages API Compatibility》：POST /anthropic/v1/messages
            ProviderOffering::anthropic("https://api.xiaomimimo.com/anthropic"),
        ],
        plans: MIMO_TOKEN_PLAN,
    },
    ProviderPreset {
        id: "longcat",
        name_key: "provider.longcat",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://longcat.chat/platform"),
        doc_url: Some("https://longcat.chat/platform/docs/zh/api-docs"),
        badge: "L",
        catalog_provider_ids: &["longcat"],
        offerings: &[
            // 官方《API 概述》：OpenAI 兼容 POST /openai/v1/chat/completions
            ProviderOffering::chat("https://api.longcat.chat/openai/v1"),
            // 官方《API 概述》：Anthropic 兼容 POST /anthropic/v1/messages
            ProviderOffering::anthropic("https://api.longcat.chat/anthropic"),
        ],
        plans: &[],
    },
    ProviderPreset {
        id: "tokenhub",
        name_key: "provider.tokenhub",
        category: ProviderCategory::China,
        keyless: false,
        // 官方按量文档「鉴权方式」页：API KEY 在控制台获取（该页同时给出接口域名表）
        key_url: Some("https://cloud.tencent.com/document/product/1823/130078"),
        doc_url: Some("https://cloud.tencent.com/document/product/1823"),
        badge: "TH",
        catalog_provider_ids: &[
            "tencent-tokenhub",
            "tencent-coding-plan",
            "tencent-token-plan",
        ],
        offerings: &[
            // 官方《API 使用说明》：广州 https://tokenhub.tencentmaas.com（新加坡为 -intl）
            ProviderOffering::chat("https://tokenhub.tencentmaas.com/v1"),
            ProviderOffering::responses("https://tokenhub.tencentmaas.com/v1"),
            // Anthropic 协议的调用路径是 `${BASE_URL}/v1/messages`（官方《Anthropic
            // Message Protocol》字段说明），BASE_URL 为裸域名——所以这里不带 /v1。
            ProviderOffering::anthropic("https://tokenhub.tencentmaas.com"),
        ],
        plans: TOKENHUB_PLANS,
    },
    ProviderPreset {
        id: "iflytek",
        name_key: "provider.iflytek",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://console.xfyun.cn/"),
        doc_url: Some("https://www.xfyun.cn/doc/spark/"),
        badge: "讯",
        catalog_provider_ids: &[],
        offerings: &[
            // 官方《HTTP 调用文档》：OpenAI 兼容 base 为 https://spark-api-open.xf-yun.com/v1
            // （X1.5 / X2-Flash 走 /v2，X2 走 /x2——换了模型可以在这里改端点）
            ProviderOffering::chat("https://spark-api-open.xf-yun.com/v1"),
        ],
        plans: IFLYTEK_CODING,
    },
    ProviderPreset {
        id: "internlm",
        name_key: "provider.internlm",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://chat.intern-ai.org.cn/"),
        doc_url: Some("https://internlm.intern-ai.org.cn/doc/"),
        badge: "书",
        catalog_provider_ids: &[],
        offerings: &[
            // 官方《Chat》：OpenAI 兼容 base 含 /api/v1
            ProviderOffering::chat("https://chat.intern-ai.org.cn/api/v1"),
            // 官方《接入 Claude Code》：POST /v1/messages，且官方明确用 x-api-key
            ProviderOffering::anthropic("https://chat.intern-ai.org.cn"),
        ],
        plans: &[],
    },
    ProviderPreset {
        id: "huawei",
        name_key: "provider.huawei",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://console.huaweicloud.com/modelarts/"),
        doc_url: Some("https://support.huaweicloud.com/model-call-maas/"),
        badge: "华",
        catalog_provider_ids: &[],
        offerings: &[
            // 官方原文：OpenAI SDK 的 base_url 设为 https://api.modelarts-maas.com/openai/v1
            // （该兼容接口目前仅「西南-贵阳一」地域开通）
            ProviderOffering::chat("https://api.modelarts-maas.com/openai/v1"),
            // 官方 Clauude Code 文档给出的 Anthropic 兼容 base
            ProviderOffering::anthropic("https://api.modelarts-maas.com/anthropic"),
        ],
        plans: HUAWEI_TOKEN_PLAN,
    },
    ProviderPreset {
        id: "jdcloud",
        name_key: "provider.jdcloud",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://console.jdcloud.com/"),
        doc_url: Some("https://docs.jdcloud.com/cn/yanxi-cap/invoke-service"),
        badge: "京",
        catalog_provider_ids: &[],
        offerings: &[
            // 官方《调用服务》：预置服务 base 为 https://modelservice.jdcloud.com/v1
            ProviderOffering::chat("https://modelservice.jdcloud.com/v1"),
            // 按量预置服务是否提供 Anthropic 面官方未说明 → 不声明，只在 TokenPlan 下声明
        ],
        plans: JDCLOUD_TOKEN_PLAN,
    },
    ProviderPreset {
        id: "antling",
        name_key: "provider.antling",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://developer.ant-ling.com/zh-CN/docs/getting-started/quickstart/"),
        doc_url: Some("https://developer.ant-ling.com/zh-CN/docs/api-reference/"),
        badge: "蚂",
        catalog_provider_ids: &[],
        offerings: &[
            // 官方 OpenAI SDK 示例：base_url = https://api.ant-ling.com/v1
            // 官方 API 参考只列 OpenAI 兼容，没有 Anthropic 面 → 不声明
            ProviderOffering::chat("https://api.ant-ling.com/v1"),
        ],
        plans: &[],
    },
    ProviderPreset {
        id: "telecom",
        name_key: "provider.telecom",
        category: ProviderCategory::China,
        keyless: false,
        key_url: Some("https://www.ctyun.cn/document/11061839"),
        doc_url: Some("https://www.ctyun.cn/document/11061839/11062312"),
        badge: "天",
        catalog_provider_ids: &[],
        offerings: &[
            // 天翼云星辰 TokenHub：base 含 /v1（海外为 ai.ctaigw.com/{region}/v1）
            ProviderOffering::chat("https://ai.ctaigw.cn/v1"),
            // 官方《Anthropic 兼容》逐字给出 POST /v1/messages + x-api-key
            ProviderOffering::anthropic("https://ai.ctaigw.cn"),
        ],
        plans: TELECOM_CODING,
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
        plans: &[],
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
        plans: &[],
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
        plans: &[],
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
        plans: &[],
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
        plans: &[],
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
        plans: &[],
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
        plans: &[],
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
        offerings: &[
            ProviderOffering::chat("https://api.siliconflow.cn/v1"),
            // 官方《创建对话请求（Anthropic）》端点为 https://api.siliconflow.cn/v1/messages，
            // Claude Code 接入页写 ANTHROPIC_BASE_URL="https://api.siliconflow.cn/"
            // ——官方文档中没有 /anthropic 这一层路径，base 就是裸 host。
            ProviderOffering::anthropic("https://api.siliconflow.cn"),
        ],
        plans: &[],
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
        offerings: &[
            ProviderOffering::chat("https://api-inference.modelscope.cn/v1"),
            // 官方文档「Responses API」节：仅支持 create，且模型目前限 Qwen 系列
            ProviderOffering::responses("https://api-inference.modelscope.cn/v1"),
            // 官方《API-Inference》「大语言模型 LLM（Anthropic API 兼容接口）」一节
            // 给出 base_url = https://api-inference.modelscope.cn（标注 beta），
            // 同样没有 /anthropic 这一层路径。
            ProviderOffering::anthropic("https://api-inference.modelscope.cn"),
        ],
        plans: &[],
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
        offerings: &[
            ProviderOffering::chat("https://openrouter.ai/api/v1"),
            ProviderOffering::responses("https://openrouter.ai/api/v1"),
            // 官方 Anthropic 接入示例：ANTHROPIC_BASE_URL=https://openrouter.ai/api（注意不是 /api/v1）
            ProviderOffering::anthropic("https://openrouter.ai/api"),
        ],
        plans: &[],
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
        offerings: &[
            // 官方现行文档只用 api.together.ai；旧的 api.together.xyz 在文档中已不再出现
            ProviderOffering::chat("https://api.together.ai/v1"),
        ],
        plans: &[],
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
        offerings: &[
            ProviderOffering::chat("https://api.fireworks.ai/inference/v1"),
            ProviderOffering::responses("https://api.fireworks.ai/inference/v1"),
            ProviderOffering::anthropic("https://api.fireworks.ai/inference"),
        ],
        plans: &[],
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
        plans: &[],
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
        plans: &[],
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
        offerings: &[
            ProviderOffering::chat("http://localhost:11434/v1"),
            ProviderOffering::responses("http://localhost:11434/v1"),
            ProviderOffering::anthropic("http://localhost:11434"),
        ],
        plans: &[],
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
        offerings: &[
            ProviderOffering::chat("http://localhost:1234/v1"),
            ProviderOffering::responses("http://localhost:1234/v1"),
            ProviderOffering::anthropic("http://localhost:1234"),
        ],
        plans: &[],
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
        offerings: &[
            ProviderOffering::chat("http://localhost:8000/v1"),
            ProviderOffering::responses("http://localhost:8000/v1"),
            ProviderOffering::anthropic("http://localhost:8000"),
        ],
        plans: &[],
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
        plans: &[],
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
                _ => {
                    if PRESETS_WITHOUT_CATALOG.contains(&preset.id) {
                        assert!(
                            !preset.has_catalog_source(),
                            "{} 已经有目录键了，请从 PRESETS_WITHOUT_CATALOG 移除",
                            preset.id
                        );
                    } else {
                        assert!(
                            preset.has_catalog_source(),
                            "{} 声明了厂商却没有目录匹配键，推荐模型会永远为空",
                            preset.id
                        );
                    }
                }
            }
        }
    }

    /// 例外名单不能有僵尸条目：每个 id 必须是真的预置、且真的没有目录键。
    #[test]
    fn the_no_catalog_exception_list_has_no_stale_entries() {
        for id in PRESETS_WITHOUT_CATALOG {
            let preset = preset_by_id(id).unwrap_or_else(|| panic!("例外名单里的 {id} 不是预置"));
            assert!(
                !preset.has_catalog_source(),
                "{id} 已有目录键，请移出例外名单"
            );
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

    /// 订阅套餐的形状不变量：与默认 offerings 同标准，且必须标记为订阅制。
    #[test]
    fn subscription_plans_are_well_formed() {
        let mut plans_seen = 0;
        for preset in BUILTIN_PRESETS {
            for plan in preset.plans {
                plans_seen += 1;
                assert!(
                    !plan.offerings.is_empty(),
                    "{} 的套餐 {} 没有端点",
                    preset.id,
                    plan.id
                );
                assert!(
                    plan.subscription,
                    "{} 的套餐 {} 必须标记为订阅制",
                    preset.id, plan.id
                );
                let mut seen: Vec<ProtocolKind> = Vec::new();
                for offering in plan.offerings {
                    assert!(
                        !seen.contains(&offering.protocol),
                        "{} 的套餐 {} 重复声明协议 {:?}",
                        preset.id,
                        plan.id,
                        offering.protocol
                    );
                    seen.push(offering.protocol);
                    assert!(
                        !offering.default_endpoint.ends_with('/'),
                        "{} 的套餐 {} 端点带结尾斜杠: {}",
                        preset.id,
                        plan.id,
                        offering.default_endpoint
                    );
                    assert!(
                        offering.default_endpoint.starts_with("https://"),
                        "{} 的套餐 {} 端点应为 https: {}",
                        preset.id,
                        plan.id,
                        offering.default_endpoint
                    );
                }
            }
            // 计费方式索引、名称 key 与订阅标记必须自洽
            for i in 0..preset.plan_count() {
                assert!(!preset.plan_name_key(i).is_empty());
                assert_eq!(
                    preset.plan_is_subscription(i),
                    i > 0,
                    "{} 的计费方式订阅标记不对",
                    preset.id
                );
            }
        }
        assert!(plans_seen > 0, "应至少有一家厂商提供订阅套餐");
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
