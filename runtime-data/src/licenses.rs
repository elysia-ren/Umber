//! 许可证门禁与来源清单（规格 X.10：许可与来源必须进入数据供应链）。
//!
//! 原则：**允许再分发的数据才进入 Bundled Catalog。**
//! 不能因为某项目公开提供 API 或 GitHub 文件，就默认可以把全部数据
//! 复制进商业软件。不满足条件的来源可以用于构建时参考，但不进安装包。

use serde::{Deserialize, Serialize};

/// 数据来源的许可与版本记录（进 Catalog，随包分发）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceLicense {
    pub source: String,
    pub snapshot: String,
    pub license: String,
    pub url: String,
    pub retrieved_at_unix: u64,
}

/// 允许再分发（进 Bundled Catalog）的许可证白名单。
pub const REDISTRIBUTABLE: &[&str] = &[
    "MIT",
    "Apache-2.0",
    "CC0-1.0",
    "CC-BY-4.0",
    "CC-BY-SA-4.0",
    "BSD-3-Clause",
];

/// 仅可用于构建时参考、不得随包分发的许可证。
pub const REFERENCE_ONLY: &[&str] = &["Proprietary", "CC-BY-NC-4.0", "unknown"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LicenseDecision {
    /// 可进入 Bundled Catalog。
    Distributable,
    /// 仅可用于构建时参考，**不得写入随包 Catalog**。
    ReferenceOnly { reason: String },
}

/// 判定某个来源能否随包分发。
pub fn decide(license: &str) -> LicenseDecision {
    let normalized = license.trim();
    if REDISTRIBUTABLE
        .iter()
        .any(|l| l.eq_ignore_ascii_case(normalized))
    {
        return LicenseDecision::Distributable;
    }
    if REFERENCE_ONLY
        .iter()
        .any(|l| l.eq_ignore_ascii_case(normalized))
    {
        return LicenseDecision::ReferenceOnly {
            reason: format!("license `{normalized}` is not in the redistribution allowlist"),
        };
    }
    LicenseDecision::ReferenceOnly {
        reason: format!("license `{normalized}` is not reviewed"),
    }
}

/// 内置来源的许可声明（构建时由维护者核对；这里是**已核对结果**的代码化）。
///
/// 实测核对（2026-09）：models.dev 与 LiteLLM 均为 MIT；OpenRouter 的
/// `/api/v1/models` 是公开 API，其数据使用需遵循 OpenRouter 条款，
/// 因此按 **reference-only** 处理——可以采集用于构建参考，
/// 但默认不写入随包 Catalog，除非人工确认条款允许再分发。
pub fn builtin_licenses() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        ("models_dev", "MIT", "https://github.com/sst/models.dev"),
        ("litellm", "MIT", "https://github.com/BerriAI/litellm"),
        (
            "openrouter",
            "reference-only",
            "https://openrouter.ai/terms",
        ),
        ("official", "Provider Terms", "厂商官方文档"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mit_and_apache_are_distributable() {
        assert_eq!(decide("MIT"), LicenseDecision::Distributable);
        assert_eq!(decide(" apache-2.0 "), LicenseDecision::Distributable);
        assert_eq!(decide("CC-BY-4.0"), LicenseDecision::Distributable);
    }

    #[test]
    fn unknown_and_proprietary_are_reference_only() {
        assert!(matches!(
            decide("Proprietary"),
            LicenseDecision::ReferenceOnly { .. }
        ));
        assert!(matches!(
            decide("GPL-3.0"),
            LicenseDecision::ReferenceOnly { .. }
        ));
        assert!(matches!(decide(""), LicenseDecision::ReferenceOnly { .. }));
    }

    #[test]
    fn builtin_licenses_cover_every_source_adapter() {
        let names: Vec<&str> = builtin_licenses().iter().map(|(n, _, _)| *n).collect();
        for expected in ["models_dev", "litellm", "openrouter", "official"] {
            assert!(names.contains(&expected), "缺少 {expected} 的许可声明");
        }
    }

    #[test]
    fn openrouter_defaults_to_reference_only() {
        // 公开 API ≠ 可再分发；默认不随包
        let (_, license, _) = builtin_licenses()
            .into_iter()
            .find(|(n, _, _)| *n == "openrouter")
            .unwrap();
        assert!(matches!(
            decide(license),
            LicenseDecision::ReferenceOnly { .. }
        ));
    }
}
