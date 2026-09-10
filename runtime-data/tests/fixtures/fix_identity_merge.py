"""修身份合并：按裸模型名合并身份，provider 归属独立的 deployments 表（规格 X.1/X.18）。"""
import io

# ---------- 1) Catalog 增加 deployments ----------
P_CAT = r"C:\个人文件\API\model-runtime\runtime-model\src\catalog.rs"
c = io.open(P_CAT, encoding="utf-8").read()

old = """/// Canonical Model Catalog。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    pub format_version: u32,
    pub generated_at_unix: u64,
    #[serde(default)]
    pub sources: Vec<CatalogSource>,
    pub identities: Vec<ModelIdentity>,
    /// 身份级知识条目（`deployment` 为空）；运行时经 Discovery 绑定 Deployment。
    #[serde(default)]
    pub entries: Vec<ModelProfile>,
}"""
new = """/// Catalog 中的一条"谁能提供这个模型"记录（规格 X.18 的 Deployment 层）。
///
/// 上游目录只告诉我们 `provider X 暴露了 model Y`，**不告诉我们协议**
/// ——协议由 Endpoint 决定，不能在这里猜，所以不设 protocol 字段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogDeployment {
    /// 上游 provider 键（如 `deepseek` / `zhipuai` / `openrouter`）。
    pub provider: String,
    /// 该 provider 侧暴露的模型标识（裸模型名，与 identity 对应）。
    pub model_id: String,
}

/// Canonical Model Catalog。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    pub format_version: u32,
    pub generated_at_unix: u64,
    #[serde(default)]
    pub sources: Vec<CatalogSource>,
    pub identities: Vec<ModelIdentity>,
    /// 身份级知识条目（`deployment` 为空）；运行时经 Discovery 绑定 Deployment。
    ///
    /// 铁律：**canonical_id 在 Catalog 内唯一**——身份按裸模型名合并，
    /// provider 差异走 `deployments`，否则按 id 查表会互相覆盖。
    #[serde(default)]
    pub entries: Vec<ModelProfile>,
    /// provider → model 的归属表（用于"这家厂商提供哪些模型"）。
    #[serde(default)]
    pub deployments: Vec<CatalogDeployment>,
}"""
assert old in c, "Catalog struct not found"
c = c.replace(old, new)

# 便捷查询：某 provider 提供的模型
old_impl = """    /// 按 canonical_id 查找身份（规范化精确匹配，见 §9.1）。
    pub fn identity(&self, canonical_id: &str) -> Option<&ModelIdentity> {"""
new_impl = """    /// 某 provider 提供的模型 ID 列表（按 canonical_id 规范化后返回）。
    ///
    /// 这是"按厂商给推荐模型"的正确数据来源：**查表而不是硬编码模型名**。
    pub fn models_of_provider(&self, provider: &str) -> Vec<String> {
        let needle = provider.trim().to_lowercase();
        let mut ids: Vec<String> = self
            .deployments
            .iter()
            .filter(|d| d.provider.to_lowercase() == needle)
            .map(|d| d.model_id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// 按 canonical_id 查找身份（规范化精确匹配，见 §9.1）。
    pub fn identity(&self, canonical_id: &str) -> Option<&ModelIdentity> {"""
assert old_impl in c, "identity fn not found"
c = c.replace(old_impl, new_impl)

# 测试：deployments 空时也要能序列化 + models_of_provider 行为
c = c.replace(
    """    fn catalog_with_version(v: u32) -> Catalog {
        Catalog {
            format_version: v,
            generated_at_unix: 0,
            sources: vec![],
            identities: vec![],
            entries: vec![],
        }
    }""",
    """    fn catalog_with_version(v: u32) -> Catalog {
        Catalog {
            format_version: v,
            generated_at_unix: 0,
            sources: vec![],
            identities: vec![],
            entries: vec![],
            deployments: vec![],
        }
    }

    #[test]
    fn models_of_provider_reads_the_deployment_table() {
        let mut catalog = catalog_with_version(1);
        catalog.deployments = vec![
            CatalogDeployment {
                provider: "deepseek".into(),
                model_id: "deepseek-chat".into(),
            },
            CatalogDeployment {
                provider: "DeepSeek".into(), // 大小写不敏感匹配
                model_id: "deepseek-reasoner".into(),
            },
            CatalogDeployment {
                provider: "openrouter".into(),
                model_id: "deepseek-chat".into(), // 同一模型，另一家也提供
            },
        ];
        let deepseek = catalog.models_of_provider("deepseek");
        assert_eq!(deepseek, vec!["deepseek-chat", "deepseek-reasoner"]);
        assert_eq!(catalog.models_of_provider("openrouter").len(), 1);
        assert!(catalog.models_of_provider("nonexistent").is_empty());
    }""",
)
io.open(P_CAT, "w", encoding="utf-8", newline="\n").write(c)
print("catalog.rs patched")

# ---------- 2) pipeline：按裸 id 合并 + 收集 deployments ----------
P_PIPE = r"C:\个人文件\API\model-runtime\runtime-data\src\pipeline.rs"
p = io.open(P_PIPE, encoding="utf-8").read()

old_key = """/// 上游 provider → Canonical 身份键。
///
/// 规范化规则（§9.1：只有窄而可靠的匹配）：小写、去首尾空白；
/// provider 前缀**保留**在身份键里，因为不同 provider 的同名模型
/// 在数据层未必是同一个 Deployment——身份合并只发生在 alias 明确时。
pub fn canonical_key_of(record: &RawModelRecord) -> String {
    normalize_model_id(&record.source_key)
}"""
new_key = """/// 记录 → Canonical 身份键。
///
/// **按裸模型名归一**：`302ai/glm-4.6`、`novita/glm-4.6`、
/// `openrouter/glm-4.6` 是**同一个身份**（规格 X.1：同一模型经不同
/// 服务商/Gateway 暴露仍是同一 Identity）。provider 差异属于 Deployment，
/// 由 `deployments` 表承载（规格 X.18）。
///
/// 早期版本按完整 source key 分组，结果是同一个模型产出多条同 id 记录，
/// 下游按 id 索引会互相覆盖——这是必须避免的数据腐化。
pub fn canonical_key_of(record: &RawModelRecord) -> String {
    normalize_model_id(record.bare_model_id())
}"""
assert old_key in p, "canonical_key_of not found"
p = p.replace(old_key, new_key)

# build(): 收集 deployments
old_build = """    let candidates: Vec<FieldCandidates> = allowed.into_iter().map(|r| normalize(&r)).collect();
    let groups = group_by_identity(candidates);"""
new_build = """    // provider → model 的归属表：身份合并后 provider 信息不丢，走这张表
    let mut deployments: Vec<CatalogDeployment> = Vec::new();
    let mut seen_deployments: std::collections::BTreeSet<(String, String)> =
        std::collections::BTreeSet::new();
    for record in &allowed {
        let provider = record
            .provider_hint
            .clone()
            .or_else(|| record.organization.clone());
        let Some(provider) = provider else {
            continue;
        };
        let model_id = record.bare_model_id().to_string();
        if seen_deployments.insert((provider.to_lowercase(), model_id.clone())) {
            deployments.push(CatalogDeployment {
                provider: provider.to_lowercase(),
                model_id,
            });
        }
    }

    let candidates: Vec<FieldCandidates> = allowed.into_iter().map(|r| normalize(&r)).collect();
    let groups = group_by_identity(candidates);"""
assert old_build in p, "build candidates not found"
p = p.replace(old_build, new_build)

old_out = """        catalog: Catalog {
            format_version: 1,
            generated_at_unix,
            sources: catalog_sources,
            identities,
            entries,
        },"""
new_out = """        catalog: Catalog {
            format_version: 1,
            generated_at_unix,
            sources: catalog_sources,
            identities,
            entries,
            deployments,
        },"""
assert old_out in p, "build output not found"
p = p.replace(old_out, new_out)

# 导入 CatalogDeployment
p = p.replace(
    "use runtime_model::catalog::{Catalog, CatalogSource};",
    "use runtime_model::catalog::{Catalog, CatalogDeployment, CatalogSource};",
)

# 新增回归测试：id 唯一 + deployments 可查
p = p.replace(
    """    #[test]
    fn adapters_integrate_with_pipeline_end_to_end() {""",
    """    #[test]
    fn canonical_ids_are_unique_after_merge() {
        // 回归：同一个模型经不同 gateway 暴露，必须合并成一条身份，
        // 否则下游按 id 查表会互相覆盖（这正是早期版本的数据腐化）。
        let mut a = third_party("models_dev", "openrouter/glm-4.6");
        a.provider_hint = Some("openrouter".into());
        a.context_window = Some(200_000);
        let mut b = third_party("litellm", "novita/glm-4.6");
        b.provider_hint = Some("novita".into());
        b.context_window = Some(200_000);
        let mut c = third_party("models_dev", "zhipuai/glm-4.6");
        c.provider_hint = Some("zhipuai".into());
        c.context_window = Some(204_800);

        let out = build(vec![a, b, c], &licenses_all(), 0);
        assert_eq!(out.catalog.entries.len(), 1, "同一身份必须只有一条");
        let ids: Vec<&str> = out
            .catalog
            .entries
            .iter()
            .map(|e| e.identity.canonical_id.as_str())
            .collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "canonical_id 在 Catalog 内必须唯一");

        // 但三家 provider 的归属都保留在 deployments 里
        assert_eq!(out.catalog.models_of_provider("zhipuai"), vec!["glm-4.6"]);
        assert_eq!(out.catalog.models_of_provider("novita"), vec!["glm-4.6"]);
        assert_eq!(out.catalog.models_of_provider("openrouter"), vec!["glm-4.6"]);
    }

    #[test]
    fn adapters_integrate_with_pipeline_end_to_end() {""",
)
io.open(P_PIPE, "w", encoding="utf-8", newline="\n").write(p)
print("pipeline.rs patched")
