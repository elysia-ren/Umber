"""给 SettingsBackend 加 load/save 契约（配置入库 + 密钥进凭据存储）。"""
import io

P = r"C:\个人文件\API\model-runtime\runtime-ui\src\backend.rs"
b = io.open(P, encoding="utf-8").read()

# 1) 新增 SavedSettings / SaveReport
old = "/// 供界面展示的模型知识（Model Intelligence 的呈现层）。"
new = '''/// 已保存的配置（**不含密钥**）。
///
/// 密钥只以引用形式存在于凭据存储，回读时只能知道"有没有"，
/// 永远不把密钥值交回界面层（§32）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedSettings {
    pub provider: String,
    pub protocol: String,
    pub endpoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    /// 凭据存储里是否已有这个 provider 的密钥。
    #[serde(default)]
    pub has_api_key: bool,
}

/// 保存结果（供界面如实提示"保存了什么"）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveReport {
    /// 配置（provider/protocol/endpoint/model/上下文覆盖）是否已落盘。
    pub saved_config: bool,
    /// 密钥是否已写入凭据存储。
    pub saved_credential: bool,
    /// 凭据存储层级告警 i18n key：落到加密文件层时必须提示用户（§32.1）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_warning_key: Option<String>,
}

/// 供界面展示的模型知识（Model Intelligence 的呈现层）。'''
assert old in b, "anchor not found"
b = b.replace(old, new, 1)

# 2) trait 增加两个方法（带默认实现，宿主接入成本仍低）
old_trait_tail = """    /// 请求 URL 预览（"请求将发送到 …"）。
    ///
    /// 由后端计算而不是 UI 拼接——**保证预览与 Adapter 实际使用的 URL
    /// 出自同一处逻辑**，否则预览会随代码演进变成谎言。
    fn preview_request_url(&self, draft: &SettingsDraft) -> Option<String> {
        let _ = draft;
        None
    }
}"""
new_trait_tail = """    /// 请求 URL 预览（"请求将发送到 …"）。
    ///
    /// 由后端计算而不是 UI 拼接——**保证预览与 Adapter 实际使用的 URL
    /// 出自同一处逻辑**，否则预览会随代码演进变成谎言。
    fn preview_request_url(&self, draft: &SettingsDraft) -> Option<String> {
        let _ = draft;
        None
    }

    /// 读取已保存的配置（启动时恢复界面）。
    ///
    /// 返回 `None` 表示从未保存过——首启是正常状态，不是错误。
    fn load_settings(&self) -> Option<SavedSettings> {
        None
    }

    /// 保存配置。
    ///
    /// 职责划分（§31 §32）：
    /// - 配置项（provider/protocol/endpoint/model/上下文覆盖）→ Runtime Local DB
    /// - `api_key` → **凭据存储**，绝不写进配置
    ///
    /// `api_key` 为空表示"不改动已保存的密钥"（用户没重新输入时）。
    fn save_settings(
        &self,
        draft: &SettingsDraft,
        api_key: &str,
    ) -> Result<SaveReport, BackendError> {
        let _ = (draft, api_key);
        Err(BackendError::new(
            "settings.save_unsupported",
            "this backend does not persist settings",
        ))
    }
}"""
assert old_trait_tail in b, "trait tail not found"
b = b.replace(old_trait_tail, new_trait_tail)

# 3) 测试：默认实现不得静默成功（保存失败必须报错，否则界面会谎称已保存）
b = b.replace(
    """        let backend = Minimal;
        assert!(backend.model_info("x").is_none());
        assert!(backend
            .preview_request_url(&SettingsDraft::default())
            .is_none());
    }""",
    """        let backend = Minimal;
        assert!(backend.model_info("x").is_none());
        assert!(backend
            .preview_request_url(&SettingsDraft::default())
            .is_none());
        assert!(backend.load_settings().is_none());
        // 未实现保存的后端必须报错，**不能假装成功**（否则界面谎称已保存）
        assert!(backend
            .save_settings(&SettingsDraft::default(), "")
            .is_err());
    }

    #[test]
    fn saved_settings_never_carries_the_secret() {
        let saved = SavedSettings {
            provider: "deepseek".into(),
            protocol: "openai_chat".into(),
            endpoint: "https://api.deepseek.com/v1".into(),
            model_id: Some("deepseek-chat".into()),
            context_window: Some(128_000),
            has_api_key: true,
        };
        let json = serde_json::to_string(&saved).unwrap();
        assert!(!json.contains("sk-"), "已保存配置里不该出现密钥形态");
        assert!(json.contains("has_api_key"));
    }""",
)

io.open(P, "w", encoding="utf-8", newline="\n").write(b)
print("backend.rs patched")
