"""demo 后端实现按厂商查目录；lib 在开窗时预热推荐；补 i18n key。"""
import io

# ---------- 1) demo 后端：按厂商匹配目录 ----------
P_MAIN = r"C:\个人文件\API\model-runtime\runtime-ui-egui\src\main.rs"
m = io.open(P_MAIN, encoding="utf-8").read()

old_impl = """    fn model_info(&self, model_id: &str) -> Option<UiModelInfo> {
        lookup(&self.catalog, model_id).cloned()
    }"""
new_impl = """    fn model_info(&self, model_id: &str) -> Option<UiModelInfo> {
        lookup(&self.catalog, model_id).cloned()
    }

    /// 按厂商**查随包目录**取推荐模型（不是硬编码模型名）。
    ///
    /// 目录里没有该厂商（如百度千帆尚未收录）时返回空，
    /// 界面会提示"刷新模型列表"或让用户手填，而不是显示过时型号。
    fn recommend_models(&self, catalog_provider_ids: &[String], limit: usize) -> Vec<UiModelEntry> {
        if catalog_provider_ids.is_empty() || limit == 0 {
            return Vec::new();
        }
        let wanted: Vec<String> = catalog_provider_ids
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
            .collect()
    }"""
assert old_impl in m, "model_info impl not found"
m = m.replace(old_impl, new_impl)

# catalog 里要带上 provider（来自 identity.organization）
old_build = """        let info = UiModelInfo {
            model_id: profile.identity.canonical_id.clone(),
            display_name: if profile.display_name.is_empty() {
                None
            } else {
                Some(profile.display_name.clone())
            },"""
new_build = """        let info = UiModelInfo {
            model_id: profile.identity.canonical_id.clone(),
            display_name: if profile.display_name.is_empty() {
                None
            } else {
                Some(profile.display_name.clone())
            },
            provider: profile.identity.organization.clone(),"""
assert old_build in m, "UiModelInfo build not found"
m = m.replace(old_build, new_build)

# 同 identity 可能有多 provider：用 provider 做索引辅助（organization 已在 profile 里）
io.open(P_MAIN, "w", encoding="utf-8", newline="\n").write(m)
print("main.rs patched")

# ---------- 2) UiModelInfo 增加 provider 字段 ----------
P_BE = r"C:\个人文件\API\model-runtime\runtime-ui\src\backend.rs"
b = io.open(P_BE, encoding="utf-8").read()
old = """pub struct UiModelInfo {
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,"""
new = """pub struct UiModelInfo {
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// 归属组织 / 上游 provider 键（用于"按厂商查目录推荐"）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,"""
assert old in b
b = b.replace(old, new)
b = b.replace(
    """        Self {
            model_id: model_id.into(),
            display_name: None,
            capabilities: Vec::new(),""",
    """        Self {
            model_id: model_id.into(),
            display_name: None,
            provider: None,
            capabilities: Vec::new(),""",
)
io.open(P_BE, "w", encoding="utf-8", newline="\n").write(b)
print("backend.rs patched")

# ---------- 3) lib：开窗前预热推荐 ----------
P_LIB = r"C:\个人文件\API\model-runtime\runtime-ui-egui\src\lib.rs"
l = io.open(P_LIB, encoding="utf-8").read()
old_open = """            let mut app = SettingsApp::new(params.page, strings, params.backend);
            app.apply_theme(&cc.egui_ctx, params.theme, params.density, params.scale);
            app.install_system_fonts(&cc.egui_ctx);
            Ok(Box::new(app))"""
new_open = """            let mut app = SettingsApp::new(params.page, strings, params.backend);
            app.apply_theme(&cc.egui_ctx, params.theme, params.density, params.scale);
            app.install_system_fonts(&cc.egui_ctx);
            // 开局就把该厂商的目录推荐填上（否则首屏模型列表是空的）
            app.prime_recommendations();
            Ok(Box::new(app))"""
if old_open in l:
    l = l.replace(old_open, new_open)
    print("lib.rs primed")
else:
    print("lib.rs open pattern miss")
io.open(P_LIB, "w", encoding="utf-8", newline="\n").write(l)

# ---------- 4) i18n ----------
P_STR = r"C:\个人文件\API\model-runtime\runtime-ui\src\strings.rs"
s = io.open(P_STR, encoding="utf-8").read()
if "providers.refresh_failed" not in s:
    s = s.replace(
        '    ("providers.refresh_models", "刷新模型列表"),',
        '    ("providers.refresh_models", "刷新模型列表"),\n'
        '    ("providers.refresh_failed", "获取模型失败："),\n'
        '    ("providers.refresh_needs_key", "需先填写 API Key"),\n'
        '    ("model.source.catalog", "目录"),\n'
        '    ("model.source.discovered", "服务"),\n'
        '    ("model.source.manual", "手填"),',
    )
    s = s.replace(
        '    ("providers.refresh_models", "Refresh models"),',
        '    ("providers.refresh_models", "Refresh models"),\n'
        '    ("providers.refresh_failed", "Failed to fetch models: "),\n'
        '    ("providers.refresh_needs_key", "API key required first"),\n'
        '    ("model.source.catalog", "catalog"),\n'
        '    ("model.source.discovered", "live"),\n'
        '    ("model.source.manual", "manual"),',
    )
    io.open(P_STR, "w", encoding="utf-8", newline="\n").write(s)
    print("strings patched")
else:
    print("strings already present")
