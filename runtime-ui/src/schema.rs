//! 声明式设置页 schema（总案 §38.1 §59）。
//!
//! Core 描述"要渲染什么、如何校验"；宿主渲染。字段值只存在
//! `SettingsDraft` 中，凭据字段在 apply 时直接写入 CredentialStore
//! 引用槽，永不进入 draft 序列化产物。

use serde::{Deserialize, Serialize};

/// 字段控件类型（渲染语义，不是视觉实现）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FieldKind {
    /// 短文本。
    Text,
    /// 凭据输入（渲染为密码框；值直送 CredentialStore）。
    Secret,
    /// 布尔开关（如"主动检测模型能力"）。
    Toggle,
    /// 单选下拉（options 为 i18n key + 值）。
    Select { options: Vec<SelectOption> },
    /// 多行文本。
    Textarea,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectOption {
    pub value: String,
    pub label_key: String,
}

/// 校验规则（Core 执行，宿主可提前预检）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldSpec {
    pub id: String,
    pub label_key: String,
    #[serde(flatten)]
    pub kind: FieldKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionSpec {
    pub title_key: String,
    pub fields: Vec<FieldSpec>,
}

/// 一页设置（Provider 配置 / 模型管理各一页，§59）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsPage {
    pub id: String,
    pub title_key: String,
    pub sections: Vec<SectionSpec>,
}

/// 用户当前填写值（凭据字段值为空占位，apply 时单独取出）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SettingsDraft {
    pub values: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub field_id: String,
    pub issue_key: String,
}

impl SettingsPage {
    /// Provider 配置页（§59 功能清单的第一屏）。
    pub fn provider_settings() -> Self {
        Self {
            id: "provider".into(),
            title_key: "settings.provider.title".into(),
            sections: vec![
                SectionSpec {
                    title_key: "settings.provider.connection".into(),
                    fields: vec![
                        FieldSpec {
                            id: "provider".into(),
                            label_key: "settings.provider.provider".into(),
                            kind: FieldKind::Select {
                                options: vec![
                                    SelectOption {
                                        value: "openai".into(),
                                        label_key: "provider.openai".into(),
                                    },
                                    SelectOption {
                                        value: "anthropic".into(),
                                        label_key: "provider.anthropic".into(),
                                    },
                                    SelectOption {
                                        value: "google".into(),
                                        label_key: "provider.google".into(),
                                    },
                                    SelectOption {
                                        value: "deepseek".into(),
                                        label_key: "provider.deepseek".into(),
                                    },
                                    SelectOption {
                                        value: "custom".into(),
                                        label_key: "provider.custom".into(),
                                    },
                                ],
                            },
                            default: Some("openai".into()),
                            required: true,
                            pattern: None,
                            help_key: None,
                        },
                        FieldSpec {
                            id: "protocol".into(),
                            label_key: "settings.provider.protocol".into(),
                            kind: FieldKind::Select {
                                options: vec![
                                    SelectOption {
                                        value: "openai_chat".into(),
                                        label_key: "protocol.openai_chat".into(),
                                    },
                                    SelectOption {
                                        value: "openai_responses".into(),
                                        label_key: "protocol.openai_responses".into(),
                                    },
                                    SelectOption {
                                        value: "anthropic_messages".into(),
                                        label_key: "protocol.anthropic_messages".into(),
                                    },
                                    SelectOption {
                                        value: "gemini".into(),
                                        label_key: "protocol.gemini".into(),
                                    },
                                ],
                            },
                            default: Some("openai_chat".into()),
                            required: true,
                            pattern: None,
                            help_key: None,
                        },
                        FieldSpec {
                            id: "endpoint".into(),
                            label_key: "settings.provider.endpoint".into(),
                            kind: FieldKind::Text,
                            default: None,
                            required: true,
                            pattern: Some(r"^https?://".into()),
                            help_key: Some("settings.provider.endpoint.help".into()),
                        },
                        FieldSpec {
                            id: "api_key".into(),
                            label_key: "settings.provider.api_key".into(),
                            kind: FieldKind::Secret,
                            default: None,
                            required: false, // 免密网关 / 本地推理服务合法（§35）
                            pattern: None,
                            help_key: None,
                        },
                    ],
                },
                SectionSpec {
                    title_key: "settings.provider.probe".into(),
                    fields: vec![FieldSpec {
                        id: "active_probe".into(),
                        label_key: "settings.provider.active_probe".into(),
                        kind: FieldKind::Toggle,
                        default: Some("false".into()), // 默认关闭（§17.2）
                        required: false,
                        pattern: None,
                        help_key: Some("settings.provider.active_probe.help".into()),
                    }],
                },
            ],
        }
    }

    /// 校验草稿。Core 是唯一真值；宿主可预检但不能替代。
    /// 字段带默认值时，草稿未填写视为已满足该项。
    pub fn validate(&self, draft: &SettingsDraft) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        for section in &self.sections {
            for field in &section.fields {
                let value = draft
                    .values
                    .get(&field.id)
                    .map(|s| s.as_str())
                    .or(field.default.as_deref())
                    .unwrap_or("");
                if field.required && value.is_empty() {
                    issues.push(ValidationIssue {
                        field_id: field.id.clone(),
                        issue_key: "validation.required".into(),
                    });
                }
                if let Some(pattern) = &field.pattern {
                    if !value.is_empty() && !simple_pattern_ok(pattern, value) {
                        issues.push(ValidationIssue {
                            field_id: field.id.clone(),
                            issue_key: "validation.pattern".into(),
                        });
                    }
                }
            }
        }
        issues
    }
}

/// 极简 pattern 支持，避免引入正则依赖。仅覆盖 schema 实际使用的形态：
/// 锚定前缀 + 可选字符（`^https?://`）。完整正则校验属于宿主渲染层的可选预检。
fn simple_pattern_ok(pattern: &str, value: &str) -> bool {
    let Some(prefix) = pattern.strip_prefix('^') else {
        return value.contains(pattern);
    };
    let prefix = prefix.trim_end_matches('$');

    // 把 "https?://" 解析为 [('h',false),('t',false),('t',false),('p',false),('s',true),...]
    let mut tokens: Vec<(char, bool)> = Vec::new();
    let mut chars = prefix.chars().peekable();
    while let Some(c) = chars.next() {
        if chars.peek() == Some(&'?') {
            chars.next();
            tokens.push((c, true));
        } else {
            tokens.push((c, false));
        }
    }

    let mut rest = value;
    for (c, optional) in tokens {
        match rest.strip_prefix(c) {
            Some(remaining) => rest = remaining,
            None if optional => {}
            None => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_page_schema_covers_contract_fields() {
        let page = SettingsPage::provider_settings();
        let ids: Vec<&str> = page
            .sections
            .iter()
            .flat_map(|s| s.fields.iter().map(|f| f.id.as_str()))
            .collect();
        assert!(ids.contains(&"provider"));
        assert!(ids.contains(&"protocol"));
        assert!(ids.contains(&"endpoint"));
        assert!(ids.contains(&"api_key"));
        // 主动 Probe 默认关闭（§17.2）
        let probe = page
            .sections
            .iter()
            .flat_map(|s| s.fields.iter())
            .find(|f| f.id == "active_probe")
            .unwrap();
        assert_eq!(probe.default.as_deref(), Some("false"));
    }

    #[test]
    fn validation_enforces_required_and_pattern() {
        let page = SettingsPage::provider_settings();

        // 空草稿：有默认值的必填项（provider / protocol）不报错，
        // 仅 endpoint（无默认值）报 required
        let empty_issues = page.validate(&SettingsDraft::default());
        assert!(empty_issues
            .iter()
            .any(|i| i.field_id == "endpoint" && i.issue_key == "validation.required"));
        assert!(!empty_issues
            .iter()
            .any(|i| i.field_id == "provider" || i.field_id == "protocol"));

        // endpoint 无默认值且必填 → 缺省即为 required 问题
        let mut draft = SettingsDraft::default();
        let issues = page.validate(&draft);
        assert!(issues
            .iter()
            .any(|i| i.field_id == "endpoint" && i.issue_key == "validation.required"));

        // 填了但格式不对 → pattern 问题，不再是 required
        draft.values.insert("endpoint".into(), "ftp://x".into());
        let issues = page.validate(&draft);
        assert!(issues
            .iter()
            .any(|i| i.field_id == "endpoint" && i.issue_key == "validation.pattern"));
        assert!(!issues.iter().any(|i| i.issue_key == "validation.required"));

        // 合法端点 → 全部通过
        draft
            .values
            .insert("endpoint".into(), "https://api.deepseek.com/v1".into());
        assert!(page.validate(&draft).is_empty());

        // `s?` 可选字符：http:// 同样合法（本地推理服务场景）
        draft
            .values
            .insert("endpoint".into(), "http://localhost:11434/v1".into());
        assert!(page.validate(&draft).is_empty());
    }

    #[test]
    fn schema_serializes_for_any_host_renderer() {
        let page = SettingsPage::provider_settings();
        let json = serde_json::to_string_pretty(&page).unwrap();
        let back: SettingsPage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, page);
    }
}
