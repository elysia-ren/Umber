//! Bundled Catalog（总案 §12 §45 §51 §63）。
//!
//! 外部数据源是数据供应链，不是 Runtime 的运行时依赖：
//! Catalog 在构建期生成、随包分发，Runtime 完全离线也可运行。

use serde::{Deserialize, Serialize};

use crate::identity::ModelIdentity;
use crate::model::ModelProfile;

/// Runtime 支持的 Catalog 格式兼容区间（总案 §51）：区间外拒绝加载并提示。
pub const SUPPORTED_FORMAT_MIN: u32 = 1;
pub const SUPPORTED_FORMAT_MAX: u32 = 1;

/// 数据来源记录（总案 §62）：仅使用许可证允许再利用与分发的内容，
/// 并保留来源、版本和许可信息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSource {
    pub name: String,
    pub snapshot: String,
    pub license: String,
    pub url: String,
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
    #[serde(default)]
    pub entries: Vec<ModelProfile>,
}

/// Catalog 格式版本不兼容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogFormatError {
    pub found: u32,
    pub supported_min: u32,
    pub supported_max: u32,
}

impl std::fmt::Display for CatalogFormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unsupported catalog format_version {} (supported: {}..={})",
            self.found, self.supported_min, self.supported_max
        )
    }
}

impl std::error::Error for CatalogFormatError {}

impl Catalog {
    /// 校验 format_version 是否落在兼容区间内（总案 §51）。
    pub fn check_format_version(&self) -> Result<(), CatalogFormatError> {
        if self.format_version >= SUPPORTED_FORMAT_MIN
            && self.format_version <= SUPPORTED_FORMAT_MAX
        {
            Ok(())
        } else {
            Err(CatalogFormatError {
                found: self.format_version,
                supported_min: SUPPORTED_FORMAT_MIN,
                supported_max: SUPPORTED_FORMAT_MAX,
            })
        }
    }

    /// 按 canonical_id 查找身份（规范化精确匹配，见 §9.1）。
    pub fn identity(&self, canonical_id: &str) -> Option<&ModelIdentity> {
        let needle = crate::identity::normalize_model_id(canonical_id);
        self.identities
            .iter()
            .find(|i| crate::identity::normalize_model_id(&i.canonical_id) == needle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog_with_version(v: u32) -> Catalog {
        Catalog {
            format_version: v,
            generated_at_unix: 0,
            sources: vec![],
            identities: vec![],
            entries: vec![],
        }
    }

    #[test]
    fn rejects_format_version_outside_compat_range() {
        assert!(catalog_with_version(1).check_format_version().is_ok());
        let err = catalog_with_version(99).check_format_version().unwrap_err();
        assert_eq!(err.found, 99);
        assert_eq!(err.supported_max, SUPPORTED_FORMAT_MAX);
    }
}
