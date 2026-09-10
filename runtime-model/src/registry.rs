//! Model Registry（总案 §37）：Identity / Deployment / ModelInfo 三层，
//! 不是 `HashMap<String, Model>`。
//!
//! - Catalog 是身份级知识的来源（离线可用，总案 §47）
//! - Deployment 在 Discovery / 手动添加时建立（§36）
//! - 字段值经 Resolver 字段级仲裁（§13），候选来源可追加
//!   （Provider API / Probe / User Override）

use std::collections::BTreeMap;

use runtime_core::DeploymentId;
use serde::{Deserialize, Serialize};

use crate::catalog::Catalog;
use crate::deployment::Deployment;
use crate::identity::ModelIdentity;
use crate::model::ModelInfo;
use crate::resolver::{FieldCandidate, FieldCategory, Resolution};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelRegistry {
    deployments: Vec<Deployment>,
    /// DeploymentId → ModelInfo（deployment 字段已绑定）
    infos: BTreeMap<String, ModelInfo>,
}

impl ModelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 从 Bundled Catalog 构建身份级知识库（无 Deployment，§12）。
    pub fn from_catalog(catalog: &Catalog) -> Result<Self, crate::catalog::CatalogFormatError> {
        catalog.check_format_version()?;
        let mut registry = Self::new();
        for entry in &catalog.entries {
            let mut info = entry.clone();
            let key = entry
                .deployment
                .as_ref()
                .map(|d| d.to_string())
                .unwrap_or_else(|| format!("catalog/{}", entry.identity.canonical_id));
            info.deployment = None; // Catalog 条目是身份级知识
            registry.infos.insert(key, info);
        }
        Ok(registry)
    }

    /// Discovery 结果 → 建立 Deployment 并挂接 ModelInfo（总案 §36 流程尾段）。
    ///
    /// 若 Catalog 中存在匹配身份的知识条目，则以其为模板（能力 / 规格 / 价格），
    /// 绑定到该 Deployment；否则建立仅含身份引用的最小 ModelInfo——
    /// 能力一律 Unknown（§15），绝不假定兼容。
    pub fn register_deployment(
        &mut self,
        deployment: Deployment,
        catalog: Option<&Catalog>,
    ) -> ModelInfo {
        let key = deployment.id.to_string();
        let identity_ref = deployment.model_id.clone();
        let mut info = catalog
            .and_then(|c| {
                c.entries
                    .iter()
                    .find(|e| e.identity.matches(&identity_ref))
                    .cloned()
            })
            .unwrap_or_else(|| ModelInfo {
                identity: ModelIdentity {
                    canonical_id: identity_ref.clone(),
                    family: identity_ref,
                    version: None,
                    aliases: vec![],
                },
                deployment: None,
                display_name: String::new(),
                description: String::new(),
                capabilities: Default::default(),
                limits: Default::default(),
                modalities: Default::default(),
                reasoning: Default::default(),
                tool_support: Default::default(),
                structured_output: Default::default(),
                pricing: None,
                compatibility: Default::default(),
                evidence: vec![],
            });
        info.deployment = Some(deployment.id.clone());
        self.infos.insert(key, info.clone());
        self.deployments.push(deployment);
        info
    }

    pub fn deployments(&self) -> &[Deployment] {
        &self.deployments
    }

    pub fn info(&self, id: &DeploymentId) -> Option<&ModelInfo> {
        self.infos.get(id.as_ref())
    }

    pub fn infos(&self) -> impl Iterator<Item = &ModelInfo> {
        self.infos.values()
    }

    /// 对某 Deployment 的某字段执行仲裁（总案 §13）。
    ///
    /// 候选由调用方收集（Catalog 模板值 + Provider API 当前值 + Probe 实测 +
    /// 用户覆盖）；本方法只做字段级优先级仲裁并保留冲突标记。
    pub fn resolve_field(
        &self,
        id: &DeploymentId,
        category: FieldCategory,
        candidates: &[FieldCandidate],
    ) -> Option<Resolution> {
        let _info = self.infos.get(id.as_ref())?; // 必须是已注册的 Deployment
        crate::resolver::resolve(category, candidates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{CapabilityKind, CapabilityRecord, CapabilityStatus};
    use crate::evidence::EvidenceSource;
    use crate::resolver::FieldValue;
    use runtime_core::ids::DeploymentId;
    use std::collections::BTreeMap;

    fn deployment() -> Deployment {
        Deployment {
            id: DeploymentId::from("deepseek/official/openai_chat/deepseek-chat"),
            endpoint_id: "ep-1".into(),
            protocol: crate::deployment::ProtocolKind::OpenAiChat,
            model_id: "deepseek-chat".into(),
        }
    }

    #[test]
    fn unknown_deployment_cannot_resolve() {
        let registry = ModelRegistry::new();
        assert!(registry
            .resolve_field(&DeploymentId::from("ghost"), FieldCategory::Spec, &[])
            .is_none());
    }

    #[test]
    fn register_without_catalog_gives_unknown_capabilities() {
        let mut registry = ModelRegistry::new();
        let info = registry.register_deployment(deployment(), None);
        // 未记录能力一律 Unknown，不假定兼容（§15 §16）
        assert_eq!(
            info.capability_status(CapabilityKind::ToolCall),
            CapabilityStatus::Unknown
        );
        assert!(info.pricing.is_none());
    }

    #[test]
    fn catalog_template_flows_into_deployment() {
        let mut capabilities = BTreeMap::new();
        capabilities.insert(
            CapabilityKind::ToolCall,
            CapabilityRecord::new(CapabilityStatus::Supported, EvidenceSource::OfficialDocs),
        );
        let catalog_entry = ModelInfo {
            identity: ModelIdentity {
                canonical_id: "deepseek-chat".into(),
                family: "deepseek".into(),
                version: None,
                aliases: vec!["deepseek_v3".into()],
            },
            deployment: None,
            display_name: "DeepSeek Chat".into(),
            description: String::new(),
            capabilities,
            limits: Default::default(),
            modalities: Default::default(),
            reasoning: Default::default(),
            tool_support: Default::default(),
            structured_output: Default::default(),
            pricing: None,
            compatibility: Default::default(),
            evidence: vec![],
        };
        let catalog = Catalog {
            format_version: 1,
            generated_at_unix: 0,
            sources: vec![],
            identities: vec![catalog_entry.identity.clone()],
            entries: vec![catalog_entry],
        };

        let mut registry = ModelRegistry::new();
        let info = registry.register_deployment(deployment(), Some(&catalog));
        assert_eq!(
            info.capability_status(CapabilityKind::ToolCall),
            CapabilityStatus::Supported
        );
        assert_eq!(
            info.deployment.as_ref().map(|d| d.as_ref()),
            Some("deepseek/official/openai_chat/deepseek-chat")
        );
    }

    #[test]
    fn field_resolution_via_registry() {
        use crate::resolver::FieldCandidate;
        let mut registry = ModelRegistry::new();
        registry.register_deployment(deployment(), None);
        let resolution = registry
            .resolve_field(
                &deployment().id,
                FieldCategory::Behavior,
                &[
                    FieldCandidate::new(FieldValue::Bool(false), EvidenceSource::OfficialDocs),
                    FieldCandidate::new(
                        FieldValue::Bool(true),
                        EvidenceSource::Probe { tested_at_unix: 1 },
                    ),
                ],
            )
            .unwrap();
        assert_eq!(resolution.value, FieldValue::Bool(true));
        assert!(resolution.conflict);
    }
}
