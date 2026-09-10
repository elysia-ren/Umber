"""demo：推荐模型改走 Catalog.deployments（provider → 模型）表。"""
import io

P = r"C:\个人文件\API\model-runtime\runtime-ui-egui\src\main.rs"
m = io.open(P, encoding="utf-8").read()

# 1) RealBackend 增加 provider → models 索引
m = m.replace(
    """struct RealBackend {
    transport: Arc<dyn HttpTransport>,
    /// 模型知识：canonical_id（规范化小写）→ 展示用知识。
    catalog: HashMap<String, UiModelInfo>,
}""",
    """struct RealBackend {
    transport: Arc<dyn HttpTransport>,
    /// 模型知识：canonical_id（规范化小写）→ 展示用知识。
    catalog: HashMap<String, UiModelInfo>,
    /// provider → 该 provider 暴露的模型 ID（规格 X.18 的 Deployment 表）。
    /// 推荐模型据此得出——**查表，不是硬编码模型名**。
    provider_models: HashMap<String, Vec<String>>,
}""",
)

m = m.replace(
    """        Self {
            transport: Arc::new(RealHttpTransport::new(HttpConfig::default())),
            catalog,
        }
    }""",
    """        Self {
            transport: Arc::new(RealHttpTransport::new(HttpConfig::default())),
            catalog,
            provider_models: load_provider_models_from_env(),
        }
    }""",
)

# 2) recommend_models 改查 provider_models
old = """        let wanted: Vec<String> = catalog_provider_ids
            .iter()
            .map(|id| id.to_lowercase())
            .collect();
        let mut hits: Vec<&UiModelInfo> = self
            .catalog
            .values()
            .filter(|info| {
                info.provider
                    .as_deref()
                    .map(|p| wanted.iter().any(|w| w == &p.to_lowercase()))
                    .unwrap_or(false)
            })
            .collect();
        // 稳定顺序：先按上下文窗口大的（通常更新更强），再按名字
        hits.sort_by(|a, b| {
            b.limits
                .context_window
                .unwrap_or(0)
                .cmp(&a.limits.context_window.unwrap_or(0))
                .then_with(|| a.model_id.cmp(&b.model_id))
        });
        hits.dedup_by(|a, b| a.model_id == b.model_id);
        hits.into_iter()
            .take(limit)
            .map(|info| UiModelEntry {
                model_id: info.model_id.clone(),
                display_name: info.display_name.clone(),
            })
            .collect()"""
new = """        let mut ids: Vec<String> = Vec::new();
        for provider in catalog_provider_ids {
            if let Some(models) = self.provider_models.get(&provider.to_lowercase()) {
                ids.extend(models.iter().cloned());
            }
        }
        ids.sort();
        ids.dedup();

        // 排序：上下文窗口大的在前（通常更强/更新），再按名字，保证稳定
        ids.sort_by(|a, b| {
            let ctx = |id: &str| {
                crate::lookup(&self.catalog, id)
                    .and_then(|info| info.limits.context_window)
                    .unwrap_or(0)
            };
            ctx(b).cmp(&ctx(a)).then_with(|| a.cmp(b))
        });

        ids.into_iter()
            .take(limit)
            .map(|model_id| {
                let display_name = crate::lookup(&self.catalog, &model_id)
                    .and_then(|info| info.display_name.clone());
                UiModelEntry {
                    model_id,
                    display_name,
                }
            })
            .collect()"""
assert old in m, "recommend_models body not found"
m = m.replace(old, new)

# 3) 加载 provider_models
old_loader_tail = """    let mut map = HashMap::new();
    for profile in &catalog.entries {"""
new_loader_tail = """    let mut map = HashMap::new();
    for profile in &catalog.entries {"""
m = m.replace(
    old_loader_tail,
    new_loader_tail,
)

# 在文件末尾加一个独立的加载器（复用同一份构建产物文件）
m = m.replace(
    """fn main() {
    let backend = RealBackend::new();""",
    """/// 从构建产物加载 provider → 模型 的归属表。
fn load_provider_models_from_env() -> HashMap<String, Vec<String>> {
    let Ok(path) = std::env::var("UMER_DEMO_CATALOG") else {
        return HashMap::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return HashMap::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return HashMap::new();
    };
    let catalog_value = value.get("catalog").cloned().unwrap_or(value);
    let Ok(catalog) = serde_json::from_value::<Catalog>(catalog_value) else {
        return HashMap::new();
    };
    let mut index: HashMap<String, Vec<String>> = HashMap::new();
    for deployment in &catalog.deployments {
        index
            .entry(deployment.provider.to_lowercase())
            .or_default()
            .push(deployment.model_id.clone());
    }
    index
}

fn main() {
    let backend = RealBackend::new();""",
)

io.open(P, "w", encoding="utf-8", newline="\n").write(m)
print("main.rs patched")
