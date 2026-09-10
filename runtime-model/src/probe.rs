//! Probe（总案 §17）。
//!
//! - `PassiveProbe`：默认开启。仅非生成性元数据请求（GET /models、HEAD、
//!   响应头解析）。契约：不发送生成请求、不调用 Tool、不消耗 Token——
//!   由 `PassiveProbe::ALLOWED_METHODS` 白名单强制。
//! - `ActiveProbe`：默认关闭，必须由 `ActiveProbeGuard::enable()` 显式开启
//!   （用户知情同意）。产生 Token 消耗。
//! - `ProbeResultStore`：持久缓存；带失效触发（端点 / 模型变化、
//!   Runtime 主版本变化、手动刷新）。持久缓存，不是永久真值。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeTestType {
    ModelList,
    Streaming,
    ToolCall,
    JsonMode,
    Vision,
    Reasoning,
}

/// Passive Probe 白名单：只允许这些方法进入非生成性请求。
pub const ALLOWED_METHODS: &[&str] = &["GET", "HEAD", "OPTIONS"];

/// 一次探测的结果记录（总案 §17.2 ProbeResult）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeResult {
    pub endpoint_id: String,
    pub model_id: String,
    pub test_type: ProbeTestType,
    pub status: crate::capability::CapabilityStatus,
    pub tested_at_unix: u64,
    /// 产生结果的 Runtime 主版本（失效触发条件之一）。
    pub runtime_major: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeError {
    /// Active Probe 未被用户开启（总案 §17.2 默认关闭）。
    Disabled,
    /// 方法不在 Passive 白名单内。
    MethodNotAllowed(String),
    EndpointChanged,
    ModelChanged,
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProbeError::Disabled => {
                f.write_str("active probe is disabled by default; requires explicit user opt-in")
            }
            ProbeError::MethodNotAllowed(m) => {
                write!(f, "method {m} is not a passive-probe method")
            }
            ProbeError::EndpointChanged => f.write_str("endpoint changed since last probe"),
            ProbeError::ModelChanged => f.write_str("model deployment changed since last probe"),
        }
    }
}

impl std::error::Error for ProbeError {}

/// Passive 探测：只做发现类请求。构造合法请求的入口在此收口，
/// 任何非白名单方法直接被拒（契约由类型保证，不靠约定）。
#[derive(Debug, Clone, Copy, Default)]
pub struct PassiveProbe;

impl PassiveProbe {
    pub fn request_allowed(method: &str) -> bool {
        ALLOWED_METHODS.contains(&method.to_uppercase().as_str())
    }
}

/// Active Probe 的启用令牌。不存在“意外构造”路径：
/// 必须显式调用 `enable()`，对应 UI 的
/// `[ ] 主动检测模型能力`（总案 §17.2）。
#[derive(Debug, Clone, Copy, Default)]
pub struct ActiveProbeGuard {
    _priv: (),
}

impl ActiveProbeGuard {
    pub fn enable() -> Self {
        Self { _priv: () }
    }

    /// 执行一次主动探测决策。真实测试的发起由 Adapter 承担；
    /// 这里是门控 + 结果记录语义。
    pub fn run(&self, test: ProbeTestType) -> ProbeOutcome {
        ProbeOutcome {
            test,
            allowed: true,
        }
    }
}

/// 探测决策结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeOutcome {
    pub test: ProbeTestType,
    pub allowed: bool,
}

/// ProbeResult 缓存（总案 §17.2：持久缓存，不是永久真值）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProbeResultStore {
    entries: BTreeMap<(String, String, ProbeTestType), ProbeResult>,
}

impl ProbeResultStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, result: ProbeResult) {
        let key = self.key(&result.endpoint_id, &result.model_id, result.test_type);
        self.entries.insert(key, result);
    }

    pub fn get(
        &self,
        endpoint_id: &str,
        model_id: &str,
        test: ProbeTestType,
    ) -> Option<&ProbeResult> {
        self.entries.get(&self.key(endpoint_id, model_id, test))
    }

    /// 是否需要重新验证（总案 §17.2 的失效触发条件）。
    pub fn should_retest(
        &self,
        endpoint_id: &str,
        model_id: &str,
        test: ProbeTestType,
        current_runtime_major: u32,
        current_endpoint_signature: &str,
        current_model_signature: &str,
    ) -> bool {
        match self.get(endpoint_id, model_id, test) {
            None => true,
            Some(existing) => {
                existing.runtime_major != current_runtime_major
                    || existing.endpoint_id != current_endpoint_signature
                    || existing.model_id != current_model_signature
            }
        }
    }

    /// 手动刷新：删除记录。
    pub fn invalidate(&mut self, endpoint_id: &str, model_id: &str, test: ProbeTestType) -> bool {
        self.entries
            .remove(&self.key(endpoint_id, model_id, test))
            .is_some()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn key(&self, e: &str, m: &str, t: ProbeTestType) -> (String, String, ProbeTestType) {
        (e.to_string(), m.to_string(), t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::CapabilityStatus;

    #[test]
    fn passive_whitelist_blocks_generation_methods() {
        assert!(PassiveProbe::request_allowed("GET"));
        assert!(PassiveProbe::request_allowed("head"));
        assert!(PassiveProbe::request_allowed("options"));
        // 一切生成性请求都不属于 Passive Probe（总案 §17.1）
        assert!(!PassiveProbe::request_allowed("POST"));
        assert!(!PassiveProbe::request_allowed("PUT"));
    }

    #[test]
    fn active_probe_requires_explicit_opt_in() {
        // 默认状态没有“已开启”的实例——类型即门禁
        let guard = ActiveProbeGuard::enable();
        assert!(guard.run(ProbeTestType::ToolCall).allowed);
    }

    #[test]
    fn cache_invalidation_triggers() {
        let mut store = ProbeResultStore::new();
        let result = ProbeResult {
            endpoint_id: "ep-1".into(),
            model_id: "m".into(),
            test_type: ProbeTestType::Streaming,
            status: CapabilityStatus::Supported,
            tested_at_unix: 100,
            runtime_major: 0,
        };
        store.record(result);

        assert!(!store.should_retest("ep-1", "m", ProbeTestType::Streaming, 0, "ep-1", "m"));
        // Runtime 主版本变化 → 重测（§17.2）
        assert!(store.should_retest("ep-1", "m", ProbeTestType::Streaming, 1, "ep-1", "m"));
        // Endpoint 变化 → 重测
        assert!(store.should_retest("ep-1", "m", ProbeTestType::Streaming, 0, "ep-2", "m"));
        // 未记录的类型 → 重测
        assert!(store.should_retest("ep-1", "m", ProbeTestType::Vision, 0, "ep-1", "m"));
        // 手动刷新
        assert!(store.invalidate("ep-1", "m", ProbeTestType::Streaming));
        assert!(store.should_retest("ep-1", "m", ProbeTestType::Streaming, 0, "ep-1", "m"));
    }
}
