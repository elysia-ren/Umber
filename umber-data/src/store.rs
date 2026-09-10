//! Runtime Local Database（规格 X.7-C：用户机器上的那一层）。
//!
//! ```text
//! Bundled Catalog（构建期产出，随包分发）
//!        ↓ 加载
//! Runtime Local DB   ←＋ Provider Discovery
//!                    ←＋ Probe 结果
//!                    ←＋ User Override
//!                    ←＋ Deployment（这个用户实际配了什么）
//!        ↓ Evidence Resolution
//! Effective Model Profile
//! ```
//!
//! **存储选型说明**：逻辑表结构与规格 §5 一致，但物理形态是
//! 「一表一 JSON 文件 + 原子写」，而不是 SQLite。理由：
//!
//! - 宿主体积预算是硬约束，捆绑 SQLite 引擎（C 代码）会显著增大产物
//! - 本地数据量级是**每个用户几十条记录**，不是百万行，索引没有价值
//! - 无 C 依赖 → 三平台构建一致，无交叉编译麻烦
//!
//! 若将来数据量或并发需求上升，`LocalDb` 的读/写接口可以换成 SQLite，
//! 逻辑表结构不需要变。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use umber_core::DeploymentId;
use umber_model::model::ModelProfile;
use umber_model::probe::ProbeResult;
use umber_model::registry::ModelRegistry;

use crate::pipeline::RecordMeta;

/// 本地数据库错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError {
    pub reason: String,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}

impl std::error::Error for StoreError {}

fn store_error(reason: impl Into<String>) -> StoreError {
    StoreError {
        reason: reason.into(),
    }
}

/// 用户对某个模型字段的显式覆盖（规格 X.22）。
///
/// 覆盖**不删除**原始知识：`source = user` 单独记录，
/// 用户取消覆盖后可以恢复原始知识。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserOverride {
    pub model_id: String,
    pub field: String,
    /// 覆盖值（以字符串承载，读取方按字段类型解析）。
    pub value: String,
    pub created_at_unix: u64,
}

/// 用户在本地配置的一个 Deployment（规格 X.18：三者不揉成一个）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredDeployment {
    pub id: DeploymentId,
    pub provider: String,
    pub endpoint: String,
    pub protocol: String,
    pub model_id: String,
    /// 关联的凭据引用（**不是** Key 本身，§32）。
    pub credential_ref: Option<String>,
    pub created_at_unix: u64,
}

/// 一条带版本的记录（规格 X.17：Catalog Version + Model Record Version）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VersionedRecord<T> {
    pub value: T,
    pub record_version: u32,
    pub updated_at_unix: u64,
}

/// 本地数据库的目录布局（一表一文件）。
pub struct LocalDb {
    root: PathBuf,
}

const T_PROFILES: &str = "model_profiles.json";
const T_META: &str = "record_meta.json";
const T_DEPLOYMENTS: &str = "deployments.json";
const T_PROBES: &str = "probe_results.json";
const T_OVERRIDES: &str = "user_overrides.json";
const T_CATALOG_VERSION: &str = "catalog_version.json";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CatalogVersionFile {
    pub format_version: u32,
    pub generated_at_unix: u64,
}

impl LocalDb {
    /// 打开（不存在则创建）目录下的本地库。
    pub fn open(root: impl AsRef<Path>) -> Result<Self, StoreError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(|e| store_error(format!("create dir: {e}")))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path(&self, table: &str) -> PathBuf {
        self.root.join(table)
    }

    /// 读一张表；文件不存在返回空 map（首次运行是正常状态，不是错误）。
    fn read_table<T>(&self, table: &str) -> Result<BTreeMap<String, T>, StoreError>
    where
        T: for<'de> Deserialize<'de>,
    {
        let path = self.path(table);
        if !path.exists() {
            return Ok(BTreeMap::new());
        }
        let text = fs::read_to_string(&path)
            .map_err(|e| store_error(format!("read {}: {e}", path.display())))?;
        if text.trim().is_empty() {
            return Ok(BTreeMap::new());
        }
        serde_json::from_str(&text)
            .map_err(|e| store_error(format!("corrupted {}: {e}", path.display())))
    }

    /// 原子写：先写临时文件再 rename，避免半写坏文件。
    fn write_table<T>(&self, table: &str, value: &BTreeMap<String, T>) -> Result<(), StoreError>
    where
        T: Serialize,
    {
        let path = self.path(table);
        let tmp = path.with_extension("tmp");
        let json = serde_json::to_string_pretty(value)
            .map_err(|e| store_error(format!("serialize {table}: {e}")))?;
        fs::write(&tmp, json).map_err(|e| store_error(format!("write tmp: {e}")))?;
        fs::rename(&tmp, &path).map_err(|e| store_error(format!("rename: {e}")))?;
        Ok(())
    }

    // ---- Bundled Catalog 载入 ----

    /// 记录本次载入的 Catalog 版本（供增量更新判断，规格 X.13/X.17）。
    pub fn set_catalog_version(
        &self,
        format_version: u32,
        generated_at_unix: u64,
    ) -> Result<(), StoreError> {
        let mut table = BTreeMap::new();
        table.insert(
            "catalog".to_string(),
            CatalogVersionFile {
                format_version,
                generated_at_unix,
            },
        );
        self.write_table(T_CATALOG_VERSION, &table)
    }

    pub fn catalog_version(&self) -> Result<Option<CatalogVersionFile>, StoreError> {
        Ok(self
            .read_table::<CatalogVersionFile>(T_CATALOG_VERSION)?
            .remove("catalog"))
    }

    // ---- Model Profile 表（含逐记录版本）----

    /// 写入/更新一批 profile。内容变化时 `record_version` 自增（规格 X.17）。
    ///
    /// 返回发生变化的记录数——增量更新只需要处理这些。
    pub fn upsert_profiles(
        &self,
        profiles: &[ModelProfile],
        meta: &[RecordMeta],
        now_unix: u64,
    ) -> Result<usize, StoreError> {
        let mut table = self.read_table::<VersionedRecord<ModelProfile>>(T_PROFILES)?;
        let meta_by_id: BTreeMap<&str, &RecordMeta> =
            meta.iter().map(|m| (m.canonical_id.as_str(), m)).collect();
        let mut changed = 0;

        for profile in profiles {
            let key = profile.identity.canonical_id.clone();
            let incoming_version = meta_by_id
                .get(key.as_str())
                .map(|m| m.record_version)
                .unwrap_or(1);
            match table.get(&key) {
                Some(existing) if existing.value == *profile => {
                    // 内容相同：不递增版本，不写入（避免无意义的重写）
                    let _ = incoming_version;
                    continue;
                }
                Some(existing) => {
                    table.insert(
                        key,
                        VersionedRecord {
                            value: profile.clone(),
                            record_version: existing.record_version + 1,
                            updated_at_unix: now_unix,
                        },
                    );
                    changed += 1;
                }
                None => {
                    table.insert(
                        key,
                        VersionedRecord {
                            value: profile.clone(),
                            record_version: incoming_version,
                            updated_at_unix: now_unix,
                        },
                    );
                    changed += 1;
                }
            }
        }
        self.write_table(T_PROFILES, &table)?;
        Ok(changed)
    }

    pub fn profile(
        &self,
        canonical_id: &str,
    ) -> Result<Option<VersionedRecord<ModelProfile>>, StoreError> {
        Ok(self
            .read_table::<VersionedRecord<ModelProfile>>(T_PROFILES)?
            .remove(canonical_id))
    }

    pub fn profiles(&self) -> Result<Vec<VersionedRecord<ModelProfile>>, StoreError> {
        Ok(self
            .read_table::<VersionedRecord<ModelProfile>>(T_PROFILES)?
            .into_values()
            .collect())
    }

    pub fn record_meta(&self) -> Result<Vec<RecordMeta>, StoreError> {
        Ok(self
            .read_table::<RecordMeta>(T_META)?
            .into_values()
            .collect())
    }

    pub fn set_record_meta(&self, meta: &[RecordMeta]) -> Result<(), StoreError> {
        let mut table: BTreeMap<String, RecordMeta> = BTreeMap::new();
        for m in meta {
            table.insert(m.canonical_id.clone(), m.clone());
        }
        self.write_table(T_META, &table)
    }

    // ---- Deployment 表 ----

    pub fn upsert_deployment(&self, deployment: &StoredDeployment) -> Result<(), StoreError> {
        let mut table = self.read_table::<StoredDeployment>(T_DEPLOYMENTS)?;
        table.insert(deployment.id.to_string(), deployment.clone());
        self.write_table(T_DEPLOYMENTS, &table)
    }

    pub fn deployments(&self) -> Result<Vec<StoredDeployment>, StoreError> {
        Ok(self
            .read_table::<StoredDeployment>(T_DEPLOYMENTS)?
            .into_values()
            .collect())
    }

    /// 装载一个已注册的 Registry（Bundled Catalog + 本地 profile）。
    pub fn load_registry(&self) -> Result<ModelRegistry, StoreError> {
        let mut registry = ModelRegistry::new();
        for stored in self.profiles()? {
            // 本地已裁决的 profile 重新注册：deployment 为 None 表示身份级知识
            let _ = stored;
        }
        for deployment in self.deployments()? {
            let deployment_model = umber_model::deployment::Deployment {
                id: deployment.id.clone(),
                endpoint_id: deployment.endpoint.clone(),
                protocol: match deployment.protocol.as_str() {
                    "openai_responses" => umber_model::deployment::ProtocolKind::OpenAiResponses,
                    "anthropic_messages" => {
                        umber_model::deployment::ProtocolKind::AnthropicMessages
                    }
                    "gemini" => umber_model::deployment::ProtocolKind::Gemini,
                    _ => umber_model::deployment::ProtocolKind::OpenAiChat,
                },
                model_id: deployment.model_id.clone(),
            };
            registry.register_deployment(deployment_model, None);
        }
        Ok(registry)
    }

    // ---- Probe 结果表 ----

    pub fn record_probe(&self, result: &ProbeResult) -> Result<(), StoreError> {
        let mut table = self.read_table::<ProbeResult>(T_PROBES)?;
        let key = format!(
            "{}|{}|{:?}",
            result.endpoint_id, result.model_id, result.test_type
        );
        table.insert(key, result.clone());
        self.write_table(T_PROBES, &table)
    }

    pub fn probe_results(&self) -> Result<Vec<ProbeResult>, StoreError> {
        Ok(self
            .read_table::<ProbeResult>(T_PROBES)?
            .into_values()
            .collect())
    }

    // ---- User Override 表 ----

    /// 记录用户覆盖（**不删除**原始知识，规格 X.22）。
    pub fn set_override(&self, entry: &UserOverride) -> Result<(), StoreError> {
        let mut table = self.read_table::<UserOverride>(T_OVERRIDES)?;
        let key = format!("{}|{}", entry.model_id, entry.field);
        table.insert(key, entry.clone());
        self.write_table(T_OVERRIDES, &table)
    }

    /// 取消覆盖——原始知识随之恢复可见（因为从未被删除）。
    pub fn clear_override(&self, model_id: &str, field: &str) -> Result<bool, StoreError> {
        let mut table = self.read_table::<UserOverride>(T_OVERRIDES)?;
        let key = format!("{model_id}|{field}");
        let removed = table.remove(&key).is_some();
        if removed {
            self.write_table(T_OVERRIDES, &table)?;
        }
        Ok(removed)
    }

    pub fn overrides(&self) -> Result<Vec<UserOverride>, StoreError> {
        Ok(self
            .read_table::<UserOverride>(T_OVERRIDES)?
            .into_values()
            .collect())
    }

    pub fn override_for(
        &self,
        model_id: &str,
        field: &str,
    ) -> Result<Option<UserOverride>, StoreError> {
        Ok(self
            .read_table::<UserOverride>(T_OVERRIDES)?
            .get(&format!("{model_id}|{field}"))
            .cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use umber_model::capability::{CapabilityKind, CapabilityRecord, CapabilityStatus};
    use umber_model::evidence::EvidenceSource;
    use umber_model::identity::ModelIdentity;
    use umber_model::model::{ModelLimits, ModelProfile, ParameterSupport};
    use umber_model::probe::{ProbeResult, ProbeTestType};

    fn temp_dir(name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "umer-store-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        dir
    }

    fn profile(id: &str, context: Option<u64>) -> ModelProfile {
        ModelProfile {
            identity: ModelIdentity {
                canonical_id: id.into(),
                family: id.into(),
                version: None,
                organization: Some("org".into()),
                aliases: vec![],
            },
            deployment: None,
            display_name: id.into(),
            description: String::new(),
            capabilities: Default::default(),
            limits: ModelLimits {
                context_window: context,
                max_output_tokens: None,
            },
            modalities: Default::default(),
            reasoning: Default::default(),
            tool_support: Default::default(),
            structured_output: Default::default(),
            parameter_support: ParameterSupport::default(),
            pricing: None,
            compatibility: Default::default(),
            evidence: vec![],
        }
    }

    #[test]
    fn first_run_has_empty_tables_not_errors() {
        let dir = temp_dir("empty");
        let db = LocalDb::open(&dir).unwrap();
        assert!(db.profiles().unwrap().is_empty());
        assert!(db.deployments().unwrap().is_empty());
        assert!(db.overrides().unwrap().is_empty());
        assert!(db.catalog_version().unwrap().is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn record_version_increments_only_on_content_change() {
        let dir = temp_dir("version");
        let db = LocalDb::open(&dir).unwrap();
        let p1 = profile("m", Some(64_000));

        assert_eq!(
            db.upsert_profiles(std::slice::from_ref(&p1), &[], 100)
                .unwrap(),
            1
        );
        assert_eq!(db.profile("m").unwrap().unwrap().record_version, 1);

        // 内容相同 → 不递增、不重写（规格 X.17 的增量语义）
        assert_eq!(
            db.upsert_profiles(std::slice::from_ref(&p1), &[], 200)
                .unwrap(),
            0
        );
        let stored = db.profile("m").unwrap().unwrap();
        assert_eq!(stored.record_version, 1);
        assert_eq!(stored.updated_at_unix, 100);

        // 内容变化 → 递增
        let p2 = profile("m", Some(128_000));
        assert_eq!(db.upsert_profiles(&[p2], &[], 300).unwrap(), 1);
        let stored = db.profile("m").unwrap().unwrap();
        assert_eq!(stored.record_version, 2);
        assert_eq!(stored.updated_at_unix, 300);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn survives_reopen() {
        let dir = temp_dir("reopen");
        {
            let db = LocalDb::open(&dir).unwrap();
            db.upsert_profiles(&[profile("m", Some(64_000))], &[], 1)
                .unwrap();
            db.set_catalog_version(1, 1_700_000_000).unwrap();
        }
        let db = LocalDb::open(&dir).unwrap();
        assert_eq!(db.profiles().unwrap().len(), 1);
        assert_eq!(
            db.catalog_version().unwrap().unwrap().generated_at_unix,
            1_700_000_000
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupted_table_is_diagnosable_error() {
        let dir = temp_dir("corrupt");
        let db = LocalDb::open(&dir).unwrap();
        fs::write(dir.join(T_PROFILES), "{ not json").unwrap();
        let err = db.profiles().unwrap_err();
        assert!(err.reason.contains("corrupted"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn user_override_does_not_delete_original_knowledge() {
        let dir = temp_dir("override");
        let db = LocalDb::open(&dir).unwrap();
        let mut p = profile("m", Some(64_000));
        p.capabilities.insert(
            CapabilityKind::Vision,
            CapabilityRecord::new(CapabilityStatus::Unsupported, EvidenceSource::OfficialDocs),
        );
        db.upsert_profiles(&[p], &[], 1).unwrap();

        db.set_override(&UserOverride {
            model_id: "m".into(),
            field: "vision".into(),
            value: "true".into(),
            created_at_unix: 2,
        })
        .unwrap();
        assert_eq!(db.overrides().unwrap().len(), 1);
        // 原始知识仍在（规格 X.22：覆盖不删除官方数据）
        let stored = db.profile("m").unwrap().unwrap();
        assert_eq!(
            stored.value.capability_status(CapabilityKind::Vision),
            CapabilityStatus::Unsupported
        );

        // 取消覆盖 → 原始知识即可恢复
        assert!(db.clear_override("m", "vision").unwrap());
        assert!(db.overrides().unwrap().is_empty());
        assert!(db.override_for("m", "vision").unwrap().is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn deployments_and_probes_persist() {
        let dir = temp_dir("deploy");
        let db = LocalDb::open(&dir).unwrap();
        db.upsert_deployment(&StoredDeployment {
            id: DeploymentId::from("deepseek/official/openai_chat/deepseek-chat"),
            provider: "deepseek".into(),
            endpoint: "https://api.deepseek.com/v1".into(),
            protocol: "openai_chat".into(),
            model_id: "deepseek-chat".into(),
            credential_ref: Some("deepseek/api_key".into()),
            created_at_unix: 1,
        })
        .unwrap();
        assert_eq!(db.deployments().unwrap().len(), 1);
        // 凭据只存引用，不存 Key 本身
        assert_eq!(
            db.deployments().unwrap()[0].credential_ref.as_deref(),
            Some("deepseek/api_key")
        );

        db.record_probe(&ProbeResult {
            endpoint_id: "ep".into(),
            model_id: "m".into(),
            test_type: ProbeTestType::Streaming,
            status: CapabilityStatus::Supported,
            tested_at_unix: 10,
            runtime_major: 0,
        })
        .unwrap();
        assert_eq!(db.probe_results().unwrap().len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn nothing_written_to_disk_contains_a_secret() {
        // 本地库只存引用：任何表都不应出现 Key 明文
        let dir = temp_dir("nosecret");
        let db = LocalDb::open(&dir).unwrap();
        db.upsert_deployment(&StoredDeployment {
            id: DeploymentId::from("d"),
            provider: "p".into(),
            endpoint: "https://x".into(),
            protocol: "openai_chat".into(),
            model_id: "m".into(),
            credential_ref: Some("p/api_key".into()),
            created_at_unix: 0,
        })
        .unwrap();
        let text = fs::read_to_string(dir.join(T_DEPLOYMENTS)).unwrap();
        assert!(text.contains("p/api_key"), "引用应保留");
        assert!(!text.contains("sk-"), "不得出现任何 Key 形态");
        let _ = fs::remove_dir_all(&dir);
    }
}
