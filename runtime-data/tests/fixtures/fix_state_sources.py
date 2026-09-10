"""状态层改造：ModelEntry 带来源、选厂商不再塞硬编码模型、支持按厂商写推荐。"""
import io

P = r"C:\个人文件\API\model-runtime\runtime-ui\src\settings_state.rs"
s = io.open(P, encoding="utf-8").read()

# 1) ModelEntry 增加 source
old = '''/// 一行的模型条目（右侧模型列表用）。
#[derive(Debug, Clone, PartialEq)]
pub struct ModelEntry {
    pub model_id: String,
    pub display_name: Option<String>,
    /// 已知的模型知识；`None` 表示"目录里没有这个模型"（仍可正常使用）。
    pub profile: Option<UiModelInfo>,
}

impl ModelEntry {
    pub fn new(model_id: impl Into<String>, profile: Option<UiModelInfo>) -> Self {
        let model_id = model_id.into();
        Self {
            display_name: profile.as_ref().and_then(|p| p.display_name.clone()),
            model_id,
            profile,
        }
    }

    pub fn label(&self) -> String {
        self.display_name
            .clone()
            .unwrap_or_else(|| self.model_id.clone())
    }
}'''
new = '''/// 模型条目的来源（界面需要区分"目录推荐"与"从服务实时拉到"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSource {
    /// 随包目录里按厂商匹配到的。
    Catalog,
    /// 从服务商 `/models` 实时拉取的。
    Discovered,
    /// 用户手填的。
    Manual,
}

impl ModelSource {
    pub fn label_key(self) -> &'static str {
        match self {
            ModelSource::Catalog => "model.source.catalog",
            ModelSource::Discovered => "model.source.discovered",
            ModelSource::Manual => "model.source.manual",
        }
    }
}

/// 一行的模型条目（右侧模型列表用）。
#[derive(Debug, Clone, PartialEq)]
pub struct ModelEntry {
    pub model_id: String,
    pub display_name: Option<String>,
    /// 已知的模型知识；`None` 表示"目录里没有这个模型"（仍可正常使用）。
    pub profile: Option<UiModelInfo>,
    /// 这个条目从哪来——决定界面标签，也决定刷新时是否被替换。
    pub source: ModelSource,
}

impl ModelEntry {
    pub fn new(model_id: impl Into<String>, profile: Option<UiModelInfo>) -> Self {
        Self::with_source(model_id, profile, ModelSource::Manual)
    }

    pub fn with_source(
        model_id: impl Into<String>,
        profile: Option<UiModelInfo>,
        source: ModelSource,
    ) -> Self {
        let model_id = model_id.into();
        Self {
            display_name: profile.as_ref().and_then(|p| p.display_name.clone()),
            model_id,
            profile,
            source,
        }
    }

    pub fn label(&self) -> String {
        self.display_name
            .clone()
            .unwrap_or_else(|| self.model_id.clone())
    }
}'''
assert old in s, "ModelEntry not found"
s = s.replace(old, new)

# 2) select_provider 不再塞硬编码模型
old2 = '''        // 该厂商的推荐模型直接进入列表，用户不必先"获取"
        for model in first.recommended_models {
            self.models.push(ModelEntry::new(*model, None));
        }
    }'''
new2 = '''        // 注意：这里**不塞任何模型名**。推荐模型由宿主按厂商查目录后写入
        // （`apply_recommendations`），实时清单则由 `apply_discovery` 写入。
        // 早期版本在这里写死了模型名，导致用户看到一年前的型号。
    }'''
assert old2 in s, "select_provider tail not found"
s = s.replace(old2, new2)

# 3) set_protocol 也不再补推荐
old3 = '''        // 协议换了，推荐模型可能不同：把新协议下的推荐补进列表
        // （已发现的模型保留，用户不必重拉一次）
        for model in offering.recommended_models {
            if !self.models.iter().any(|m| m.model_id == *model) {
                self.models.push(ModelEntry::new(*model, None));
            }
        }
    }'''
new3 = '''        let _ = offering;
        // 同上：不在此处注入模型名。换协议后界面会重新向宿主请求推荐。
    }'''
assert old3 in s, "set_protocol tail not found"
s = s.replace(old3, new3)

# 4) add_model 保留来源；新增 apply_recommendations
old4 = '''    pub fn add_model(&mut self, entry: ModelEntry) {
        if let Some(existing) = self.models.iter_mut().find(|m| m.model_id == entry.model_id) {
            // 已存在则用更完整的知识替换
            if entry.profile.is_some() {
                *existing = entry;
            }
            return;
        }
        self.models.push(entry);
    }'''
new4 = '''    pub fn add_model(&mut self, entry: ModelEntry) {
        if let Some(existing) = self.models.iter_mut().find(|m| m.model_id == entry.model_id) {
            // 已存在：用更完整的知识替换，并保留更强的来源（实时 > 目录 > 手填）
            let merged = ModelEntry {
                model_id: entry.model_id,
                display_name: entry.display_name.or_else(|| existing.display_name.clone()),
                profile: entry.profile.or_else(|| existing.profile.clone()),
                source: if entry.profile.is_some() {
                    entry.source
                } else {
                    existing.source
                },
            };
            *existing = merged;
            return;
        }
        self.models.push(entry);
    }

    /// 写入**按厂商从目录取到的推荐模型**（宿主在切换厂商/协议后调用）。
    ///
    /// 只替换目录来源的条目：用户已发现的与手填的一律保留。
    pub fn apply_recommendations(&mut self, recommended: Vec<ModelEntry>) {
        self.models
            .retain(|m| m.source != ModelSource::Catalog);
        for mut entry in recommended {
            entry.source = ModelSource::Catalog;
            self.add_model(entry);
        }
        if self.selected_model.is_none() {
            if let Some(first) = self.models.first() {
                self.selected_model = Some(first.model_id.clone());
            }
        }
    }'''
assert old4 in s, "add_model not found"
s = s.replace(old4, new4)

# 5) apply_discovery 标记来源，并在替换时保留手填
old5 = '''    pub fn apply_discovery(&mut self, found: Vec<ModelEntry>) {
        self.discovered_count = found.len();
        for entry in found {
            self.add_model(entry);
        }
        if self.selected_model.is_none() {
            if let Some(first) = self.models.first() {
                self.selected_model = Some(first.model_id.clone());
            }
        }
    }'''
new5 = '''    pub fn apply_discovery(&mut self, found: Vec<ModelEntry>) {
        self.discovered_count = found.len();
        // 上一轮实时结果先清掉（服务端清单可能已变），目录推荐与手填保留
        self.models
            .retain(|m| m.source != ModelSource::Discovered);
        for mut entry in found {
            entry.source = ModelSource::Discovered;
            self.add_model(entry);
        }
        if self.selected_model.is_none() {
            if let Some(first) = self.models.first() {
                self.selected_model = Some(first.model_id.clone());
            }
        }
    }'''
assert old5 in s, "apply_discovery not found"
s = s.replace(old5, new5)

# 6) remove_model 也清理 queried 无关；导出 ModelSource
s = s.replace(
    "pub use settings_state::{\n    protocol_slug, resolve_context_window, ContextWindowEvidence, ModelEntry, SettingsState,\n};",
    "",
)

io.open(P, "w", encoding="utf-8", newline="\n").write(s)
print("settings_state patched")
