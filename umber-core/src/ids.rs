//! 标识符类型（总案 §23–§25）。

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! define_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_string())
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

define_id! {
    /// 一次模型调用的唯一标识（总案 §25）。
    InvocationId
}

define_id! {
    /// 内容块在 Invocation 内的归属标识，保证并行 Block 的 Delta 无歧义（总案 §24）。
    BlockId
}

define_id! {
    /// 工具调用的归属标识（总案 §24）。
    CallId
}

define_id! {
    /// Deployment 引用。请求里的 `model` 指向的是 Deployment，不是裸模型 ID（总案 §8）。
    DeploymentId
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_serializes_as_plain_string() {
        let id = InvocationId::from("inv-1");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"inv-1\"");
        let back: InvocationId = serde_json::from_str("\"inv-1\"").unwrap();
        assert_eq!(back, id);
    }
}
