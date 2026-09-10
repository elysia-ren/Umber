# 宿主集成指南

本文档面向把 Umber（Universal Embedded Model Runtime）集成进自己软件的开发者。
契约定义以代码为准（Rust 类型是唯一真值），本文只讲"怎么用"。

## 1. 最小接入（Rust）

```rust
use std::sync::Arc;
use runtime_credential::{CredentialRef, InMemoryCredentialStore, SecretString};
use runtime_core::{message::Message, request::GenerateRequest};
use runtime_engine::{run_invocation, CancelToken, RetryPolicy, TimeoutPolicy};
use runtime_model::deployment::{Deployment, Endpoint, ProtocolKind};
use runtime_protocol::OpenAiChatAdapter;
use runtime_provider::ProviderAdapter;

// 1) 凭据：引用式，秘密不落配置 JSON（总案 §32）
let credentials = InMemoryCredentialStore::new();
let key_ref = CredentialRef::from("deepseek/api_key");
credentials.set(&key_ref, SecretString::new("sk-..."))?;

// 2) Endpoint 可自由覆盖，Preset 不锁死（总案 §34）
let endpoint = Endpoint {
    id: "ep-1".into(),
    provider_id: "deepseek".into(),
    url: "https://api.deepseek.com/v1".into(),
};

// 3) Deployment 指向 Provider 侧模型 ID（总案 §8）
let deployment = Deployment {
    id: "deepseek/official/openai_chat/deepseek-chat".into(),
    endpoint_id: endpoint.id.clone(),
    protocol: ProtocolKind::OpenAiChat,
    model_id: "deepseek-chat".into(),
};

// 4) Adapter 负责协议转换；Engine 负责生命周期（总案 §28–§31）
let adapter = OpenAiChatAdapter::new(transport);
let request = GenerateRequest::new(deployment.id.clone(), vec![Message::user("你好")]);

let factory = {
    let (endpoint, deployment, request) = (endpoint.clone(), deployment.clone(), request.clone());
    move || adapter.execute(&request, &endpoint, &deployment, &credentials, &key_ref)
};

let cancel = CancelToken::new();
let mut events = Vec::new();
let outcome = run_invocation(
    &factory, &request, &cancel,
    &TimeoutPolicy::default(), &RetryPolicy::default(),
    &mut |e| events.push(e),
);

// outcome.response: Option<GenerateResponse>（含 stop_reason / content / usage）
// outcome.partial : 失败或取消后仍可取回的部分结果（总案 §25.1）
```

宿主永远不需要写 `if provider == "..."`。若你需要，说明 Runtime 边界已失败。

## 2. 事件流约定

- 所有 Delta 是 **append-only 增量**，按 `block_id` / `call_id` 归属，可乱序到达后重组（总案 §24）
- `sequence` 在 Invocation 内全局单调、由 Runtime 签发
- **每个 Invocation 恰好一个终结事件**：`Completed` / `Failed` / `Cancelled`。
  断流、畸形 SSE、超时都会由 Runtime 合成终结事件——宿主不必自己判断"流是不是死了"（§23.1）
- `stop_reason` 是判断 Agent Loop 是否继续的唯一依据：`ToolUse` 时才执行工具（§27）

## 3. 工具调用（Runtime 不执行工具，§21）

```text
模型 → ToolCall 事件（call_id / name / arguments_json 增量重组）
宿主 → 自行决定执行谁、如何沙箱
宿主 → 提交 ToolResult 块（下一轮请求的 Message）
```

多轮场景必须保留 `Reasoning` 块及其 `provider_payload`，否则部分 Provider
（Anthropic thinking 签名、OpenAI Responses 加密推理项）会拒绝后续请求（§19.1）。

## 4. 凭据

三层回退（`runtime-credential-os`，总案 §32.1）：

```rust
use runtime_credential_os::{CredentialTier, EncryptedFileStore, FallbackChain, OsKeystore};
use std::sync::Arc;

let chain = FallbackChain::new(vec![
    // 1) 宿主自己的实现（例如企业密钥管理）
    (CredentialTier::Host, host_store),
    // 2) 操作系统钥匙串：Windows Credential Manager / macOS Keychain / libsecret
    (CredentialTier::OsKeystore, Arc::new(OsKeystore::new())),
    // 3) 加密文件（ChaCha20-Poly1305）
    (CredentialTier::EncryptedFile, Arc::new(EncryptedFileStore::open(dir)?)),
]);

// 启动时探测实际生效的层级；落到 EncryptedFile 必须在 UI 上告警
if let Some(tier) = chain.probe() {
    if tier.requires_user_warning() {
        // 渲染 strings()[tier.label_key()] 给出的提示
    }
}
```

- 读操作按优先级回退；写操作默认写入所有可用层，使回退对用户透明
- `SecretString` 的 Debug/Display 输出为 `***`；`redact_json` / `redact_text`
  用于任何要写日志或诊断的原始载荷（§28）
- **加密文件层的边界**：密钥与数据同机，防的是"配置文件被顺手读走 / 同步到云盘"，
  不防本机恶意软件——所以告警是契约要求，不是可选。

## 4.1 网络与代理

`RealHttpTransport` 的代理解析顺序：

```text
显式配置（HttpConfig::proxy）
    ↓ 未设置
环境变量（ALL_PROXY / HTTPS_PROXY / HTTP_PROXY，大小写均支持）
    ↓ 未设置
系统代理（Windows：Internet 设置注册表；macOS：scutil --proxy）
```

**系统代理这一档是桌面场景的必需项**：用户在系统设置里开代理时不会设置环境变量，
缺这一档会让宿主在用户机器上莫名连不上。

## 5. 设置界面（UISpec，§38）

宿主有两种选择：

1. **渲染 UISpec**：`SettingsPage::provider_settings()` 返回纯数据 schema
   （字段、类型、校验规则、i18n key），宿主用自己的控件渲染，校验调
   `page.validate(&draft)`。任何技术栈都能走这条路。
2. **嵌入参考实现**：随 Runtime 提供的参考 UI（视觉可替换，接受
   Theme / Language / Scale / Density）。

契约冻结的是 UISpec 数据契约；视觉与控件实现不是契约。

## 6. 模型发现与手动回退

```text
adapter.discover_models(...)   // GET /models
        ↓ 失败也合法（总案 §36）
DiscoverySession::add_manual_model("model-id")
```

**没有 `/models` 不是使用模型的硬性阻断条件。**

## 7. C / 其他语言（稳定 C ABI）

```c
uint32_t v = runtime_abi_version();          /* (major << 16) | minor */
if ((v >> 16) != EXPECTED_MAJOR) { /* 拒绝启动 */ }

UmerRuntime* rt = runtime_init();

/* 真实调用：注册部署 + 写凭据（+ 可选加载 Catalog）后再拉流。
   未配置且未开 demo 时 open 返回 UMER_ERR_NOT_CONFIGURED（不静默给假数据） */
runtime_set_deployment(rt, config_json, config_len);
runtime_set_credential(rt, "deepseek/api_key", api_key);
runtime_load_catalog(rt, "catalog.json");

UmerStream* stream = NULL;
runtime_stream_open(rt, request_json, len, &stream);

UmerEvent ev;
for (;;) {
    int32_t st = runtime_stream_next(stream, 2000, &ev);
    if (st == UMER_EVENT) {
        /* ev.json 归调用方，用完必须释放 */
        runtime_string_free((char*)ev.json);
    } else if (st == UMER_WOULD_BLOCK) {
        continue;               /* 超时但流未关闭 */
    } else if (st == UMER_CLOSED) {
        break;                  /* 流已终结 */
    } else {
        break;                  /* 负数是错误码 */
    }
}
runtime_stream_close(stream);
runtime_shutdown(rt);
```

ABI 硬规则（总案 §50.2）：句柄谁分配谁释放；panic 不穿越边界（全部入口
catch_unwind）；字符串一律 UTF-8 + 显式长度；`runtime_stream_cancel` 可与
`runtime_stream_next` 并发调用。可运行示例见 `runtime-ffi/examples/host_example.c`。

ABI 0.2 起，C 宿主拿到的是**和 Rust 宿主同一条真实链路**（四个协议 Adapter +
真实 HTTP/SSE + Engine 生命周期）：

```text
runtime_set_deployment(rt, json, len)   注册 Deployment（id/protocol/endpoint_url/model_id）
runtime_set_credential(rt, ref, secret) 凭据（只在内存，Runtime 不落盘）
runtime_load_catalog(rt, path)          离线模型知识（可选）
runtime_status(rt, out)                 运行时状态 JSON（自检用）
runtime_set_demo(rt, 1)                 显式开启内置假流，仅验证 ABI 形态
```

**头文件由 Rust 类型生成**（不要手改）：

```text
cbindgen --config runtime-ffi/cbindgen.toml --crate runtime-ffi -o runtime-ffi/include/umer.h
```

### Python（`runtime-ffi/bindings/python/umer.py`）

纯标准库 `ctypes`，无第三方依赖：

```python
from umer import Runtime, UmerRuntimeFailure

with Runtime() as rt:
    with rt.stream({"model": "dep-1", "messages": [...]}) as stream:
        for event in stream:                      # 迭代到终结事件自动停止
            print(event["sequence"], event["data"]["event"]["type"])
        print(stream.partial_text)                # 失败/取消后仍可取回
```

**ctypes 的所有权陷阱**（绑定层已处理，自己写绑定时务必注意）：
`UmerEvent.json` 必须声明为 `c_void_p`，不能用 `c_char_p`——后者的字段访问会
被 ctypes 自动解引用成 Python `bytes`，把 Python 自己的缓冲区交给
`runtime_string_free` 会造成堆损坏（实测 `0xC0000374`）。

## 8. 数据供应链（不随宿主分发）

模型元数据由 `runtime-data` 的 `model-data` CLI 于构建期生成：

```text
model-data build source1.json source2.json -o catalog.json
```

许可证白名单门禁（`ALLOWED_LICENSES`）不通过即构建失败；产物随 Runtime
分发，**Runtime 完全离线也能运行**（§47 §63）。

## 9. 常见陷阱

| 陷阱 | 后果 | 正确做法 |
|------|------|----------|
| 丢弃 `Reasoning.provider_payload` | 多轮工具调用被 Provider 拒绝 | 原样保留并回放（§19.1） |
| 用 sequence 之外的顺序假设重组 Delta | 并行 ToolCall 归属错乱 | 按 `block_id` / `call_id` 归属 |
| 失败后丢弃已产出内容 | 用户看到空回复 | 用 `outcome.partial`（§25.1） |
| 把 `retryable` 当作"必须重试" | 已产出内容被重复计费 | 由 Engine 决策，宿主不干预（§29） |
| 依赖第三方 catalog 服务在线 | 断网即不可用 | 用 Bundled Catalog（§47） |
| 为每家厂商写分支 | 边界已失败 | 一切差异应在 CompatibilityProfile（§16） |
| 要求用户设置代理环境变量 | 开了系统代理的用户会连不上 | 传输层已读系统代理；自定义传输也要处理 |
