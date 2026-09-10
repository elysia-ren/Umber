//! Provider Adapter Contract（总案 §39.4 §51–§55）。
//!
//! Adapter 保持极小：`describe / discover_models / execute`。
//!
//! Adapter 负责：协议转换、Provider 请求构造、Provider 响应解析、私有协议处理。
//! Adapter 不拥有：Retry policy、Credential policy、Invocation lifecycle、
//! Capability resolver、Catalog —— 这些属于 Runtime。
//!
//! 铁律：Adapter 内部的 Provider-specific 状态不得进入 Canonical API；
//! Core 编译单元不得出现 AnthropicMessage / OpenAIResponseItem / GeminiPart。

#![forbid(unsafe_code)]

use umber_core::error::ModelError;
use umber_core::request::GenerateRequest;
use umber_credential::{CredentialRef, CredentialStore};
use umber_engine::ProviderStream;
use umber_model::deployment::{Deployment, Endpoint, ProtocolKind};

/// Provider / Protocol 能力与基础描述。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterDescriptor {
    pub provider_id: String,
    pub display_name: String,
    pub protocols: Vec<ProtocolKind>,
}

/// Discovery 发现的一个模型（Provider 侧 ID + 可选展示名）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredModel {
    pub model_id: String,
    pub display_name: Option<String>,
}

/// 极小 Adapter 接口（总案 §51）。
pub trait ProviderAdapter: Send + Sync {
    fn describe(&self) -> AdapterDescriptor;

    /// 模型列表获取 + ID 解析。没有 `/models` 不是使用的硬性阻断（§36），
    /// 失败由上层允许手动添加 Model ID。
    fn discover_models(
        &self,
        endpoint: &Endpoint,
        credentials: &dyn CredentialStore,
        credential_ref: &CredentialRef,
    ) -> Result<Vec<DiscoveredModel>, ModelError>;

    /// 接受 Canonical Request，输出 Canonical 事件流（拉取式）。
    ///
    /// `deployment` 提供协议与 Provider 侧 model_id（总案 §8：Deployment ≠
    /// ModelIdentity）；凭据由 Core 持有的仓库按引用解析，Adapter 只读取。
    fn execute(
        &self,
        request: &GenerateRequest,
        endpoint: &Endpoint,
        deployment: &Deployment,
        credentials: &dyn CredentialStore,
        credential_ref: &CredentialRef,
    ) -> Result<Box<dyn ProviderStream>, ModelError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// trait 可实现性冒烟：任何协议 Adapter 都长这个样子。
    struct NoopAdapter;

    impl ProviderAdapter for NoopAdapter {
        fn describe(&self) -> AdapterDescriptor {
            AdapterDescriptor {
                provider_id: "noop".into(),
                display_name: "Noop".into(),
                protocols: vec![ProtocolKind::OpenAiChat],
            }
        }

        fn discover_models(
            &self,
            _endpoint: &Endpoint,
            _credentials: &dyn CredentialStore,
            _credential_ref: &CredentialRef,
        ) -> Result<Vec<DiscoveredModel>, ModelError> {
            Ok(vec![])
        }

        fn execute(
            &self,
            _request: &GenerateRequest,
            _endpoint: &Endpoint,
            _deployment: &Deployment,
            _credentials: &dyn CredentialStore,
            _credential_ref: &CredentialRef,
        ) -> Result<Box<dyn ProviderStream>, ModelError> {
            Err(ModelError::Unsupported(
                umber_core::error::ErrorDetail::new("noop adapter"),
            ))
        }
    }

    #[test]
    fn adapter_trait_is_implementable_and_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<NoopAdapter>();
        let a = NoopAdapter;
        assert_eq!(a.describe().provider_id, "noop");
    }
}
