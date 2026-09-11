# Umber

[![ci](https://github.com/elysia-ren/Umber/actions/workflows/ci.yml/badge.svg)](https://github.com/elysia-ren/Umber/actions/workflows/ci.yml)

**Universal Embedded Model Runtime** — an embeddable runtime that unifies AI model
access for host applications.
Ship it inside your product and link it into your own process: it absorbs the
protocol differences, model knowledge, streaming behaviour, error taxonomy and
credential handling, so your code only ever talks to one stable Canonical API.

[中文说明 / Chinese](README.zh-CN.md)

## Why

Talking to even a handful of providers means owning several wire protocols,
several streaming dialects, several error vocabularies, several tool-call
encodings, plus retries, timeouts, cancellation and secrets. This runtime does
that once, and keeps doing it as the providers drift.

| | |
|---|---|
| **One Canonical API** | Request / Event / Response / Error / Invocation / Usage types that leak no provider specifics. Host code never branches on the provider. |
| **Four protocols, one shape** | `openai_chat`, `openai_responses`, `anthropic_messages`, `gemini` - real HTTP and SSE transports included. |
| **Streaming you can trust** | Exactly one terminal event per invocation, strictly increasing `sequence`, append-only deltas, partial results that survive failure and cancellation. |
| **Model intelligence** | An offline catalog of canonical model identities with capabilities, context limits, pricing and evidence. `unknown` stays `unknown` instead of being invented. |
| **Credentials handled properly** | Secrets never reach config files, catalogs or logs. Host store -> OS keychain -> encrypted file, with a mandatory warning when the weakest tier is active. |
| **Embeddable surface** | A stable pull-based C ABI (no async across the FFI boundary) and a dependency-free Python binding. Rust hosts link the crates directly. |

## Guarantees

These are contract-level promises, each covered by the conformance suites.

```text
Terminal event    broken stream / malformed SSE / timeout / cancellation
                  -> exactly one completed | failed | cancelled
Partial results   content and usage produced before a failure or cancellation
                  remain retrievable
Global sequence   every event carries a strictly increasing sequence; parallel
                  tool calls re-assemble by call_id / index
Four timeouts     connect / first-token / idle / total, enforced by the engine
Safe retry        a request that already produced output is never blindly replayed
Error taxonomy    14 canonical error kinds; hosts never parse HTTP status codes
Redaction         sensitive headers and JSON keys are redacted; secrets print as ***
```

## Workspace layout

```text
umber-core/           Canonical API contracts (Request / Event / Response / Error / Invocation / Usage)
umber-model/          Model intelligence (identity / deployment / capability / evidence / resolver / catalog / registry / probe)
umber-engine/         Invocation engine (finality / four-stage timeouts / retry / partial results / cancellation)
umber-provider/       Provider adapter trait
umber-protocol/       Four protocol adapters, real HTTP transport, SSE and error mapping
umber-conformance/    Conformance suite (fake provider / assertions / fixture format)
umber-credential/     CredentialStore contract, in-memory store, redaction helpers
umber-credential-os/  Platform credentials (OS keychain / encrypted file / fallback chain)
umber-ui/             UISpec contract (settings schema / validation / discovery state machine / i18n)
umber-ui-egui/        Reference settings UI (replaceable; the visual layer is not a contract)
umber-ffi/            Stable C ABI (pull-based) + C and Python bindings
umber-data/           Model data pipeline and database (the `model-data` CLI)
```

One rule is enforced mechanically: **only `umber-ffi` may use `unsafe`;
every other crate carries `#![forbid(unsafe_code)]`.**

## Quick start

### Rust

```rust
use std::sync::Arc;

use umber_core::{message::Message, request::GenerateRequest};
use umber_credential::{CredentialRef, InMemoryCredentialStore, SecretString};
use umber_engine::{run_invocation, CancelToken, RetryPolicy, TimeoutPolicy};
use umber_model::deployment::{Deployment, Endpoint, ProtocolKind};
use umber_protocol::{HttpConfig, HttpTransport, OpenAiChatAdapter, RealHttpTransport};
use umber_provider::ProviderAdapter;

// 1) Credentials are referenced, never inlined into the request.
let credentials = InMemoryCredentialStore::new();
let key_ref = CredentialRef::from("deepseek/api_key");
credentials.set(&key_ref, SecretString::new(std::env::var("DEEPSEEK_API_KEY")?))?;

// 2) Endpoint (freely overridable) and 3) Deployment (provider-side model id).
let endpoint = Endpoint {
    id: "ep-1".into(),
    provider_id: "deepseek".into(),
    url: "https://api.deepseek.com/v1".into(),
};
let deployment = Deployment {
    id: "deepseek/official/openai_chat/deepseek-chat".into(),
    endpoint_id: endpoint.id.clone(),
    protocol: ProtocolKind::OpenAiChat,
    model_id: "deepseek-chat".into(),
};

// 4) The adapter converts protocol; the engine owns the invocation lifecycle.
let transport: Arc<dyn HttpTransport> = Arc::new(RealHttpTransport::new(HttpConfig::default()));
let adapter = OpenAiChatAdapter::new(transport);
let request = GenerateRequest::new(deployment.id.clone(), vec![Message::user("Hello")]);

let factory = {
    let (endpoint, deployment, request, credentials, key_ref) = (
        endpoint.clone(),
        deployment.clone(),
        request.clone(),
        credentials,
        key_ref.clone(),
    );
    move || adapter.execute(&request, &endpoint, &deployment, &credentials, &key_ref)
};

let mut events = Vec::new();
let outcome = run_invocation(
    &factory,
    &request,
    &CancelToken::new(),
    &TimeoutPolicy::default(),
    &RetryPolicy::default(),
    &mut |event| events.push(event),
);

// outcome.response: Option<GenerateResponse>  (stop_reason / content / usage)
// outcome.partial : content and usage that survived a failure or a cancellation
```

### C / C++

The C ABI is pull-based: you ask for the next event and it blocks for at most
`timeout_ms`. Set up the runtime once, then open one stream per invocation.

```c
#include <string.h>
#include "umber.h"

if (runtime_abi_version() >> 16 != 0) { /* major mismatch: refuse to start */ }

UmerRuntime* rt = runtime_init();

/* Register the deployment: the request model must match this id exactly. */
const char* config =
    "{\"id\":\"deepseek/official/openai_chat/deepseek-chat\","
    "\"provider_id\":\"deepseek\","
    "\"protocol\":\"openai_chat\","
    "\"endpoint_url\":\"https://api.deepseek.com/v1\","
    "\"model_id\":\"deepseek-chat\","
    "\"credential_ref\":\"deepseek/api_key\"}";
runtime_set_deployment(rt, config, strlen(config));

/* Credentials live in memory only; persist them yourself (OS keychain). */
runtime_set_credential(rt, "deepseek/api_key", getenv("DEEPSEEK_API_KEY"));

/* Optional: offline model knowledge. */
runtime_load_catalog(rt, "catalog.json");

const char* req =
    "{\"model\":\"deepseek/official/openai_chat/deepseek-chat\",\"messages\":["
    "{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"Hello\"}]}]}";

UmerStream* stream = NULL;
runtime_stream_open(rt, req, strlen(req), &stream);

UmerEvent ev;
for (;;) {
    int32_t status = runtime_stream_next(stream, 2000, &ev);
    if (status == UMER_EVENT) {
        /* ev.json belongs to the caller: free it exactly once. */
        puts(ev.json);
        runtime_string_free((char*)ev.json);
    } else if (status == UMER_WOULD_BLOCK) {
        continue;               /* timed out, stream still open */
    } else if (status == UMER_CLOSED) {
        break;                  /* terminal event already delivered */
    } else {
        break;                  /* negative code: see umber.h */
    }
}
runtime_stream_close(stream);
runtime_shutdown(rt);
```

Build the bundled example (Windows / MSVC):

```bat
cl /I include examples\host_example.c /Fe:host_example.exe /link lib\umber_ffi.dll.lib
```

`runtime_stream_open` returns `UMER_ERR_NOT_CONFIGURED` when the requested
model has no registered deployment and the built-in demo source has not been
enabled: the runtime never silently returns fabricated data.

### Python

`umber-ffi/bindings/python/umber.py` uses only the standard library.

```python
from umber import Runtime

with Runtime() as rt:                       # checks the ABI major version
    rt.set_deployment({
        "id": "deepseek/official/openai_chat/deepseek-chat",
        "provider_id": "deepseek",
        "protocol": "openai_chat",
        "endpoint_url": "https://api.deepseek.com/v1",
        "model_id": "deepseek-chat",
        "credential_ref": "deepseek/api_key",
    })
    rt.set_credential("deepseek/api_key", "sk-...")
    rt.load_catalog("catalog.json")         # optional, offline

    request = {
        "model": "deepseek/official/openai_chat/deepseek-chat",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "Hello"}]}],
    }
    with rt.stream(request) as stream:
        for event in stream:                # stops at the terminal event
            inner = event["data"]["event"]
            if inner["type"] == "text_delta":
                print(inner["delta"], end="", flush=True)
```

## Reference settings UI

![settings window](umber-ui-egui/examples/wizard-dark.png)

`umber-ui-egui` is a working settings window built only on the `umber-ui`
data contract: schema-driven forms, light and dark tokens, CJK font fallback
loaded from the system, and headless frame tests. It reaches the network, the
disk and the keychain exclusively through the host-supplied `SettingsBackend`,
so replacing it never touches the contract. Its built-in catalogue covers 32
vendors across the four protocols, and each vendor exposes its billing plans
(pay-as-you-go vs. a subscription / coding plan) as endpoints of the same
deployment - switching plans never creates a second vendor entry.

## Build and test

```text
cargo test                                  # full suite
cargo clippy --all-targets -- -D warnings   # zero-warning gate
cargo fmt                                   # formatting

# C ABI example (Windows / MSVC)
umber-ffi\examples\build_example.cmd debug

# regenerate the C header from the Rust types (the single source of truth)
cbindgen --config umber-ffi/cbindgen.toml --crate umber-ffi -o umber-ffi/include/umber.h
```

On Linux, `umber-credential-os` reaches the Secret Service API through
`libdbus`, so the build needs `libdbus-1-dev` and `pkg-config`:

```bash
sudo apt-get install -y libdbus-1-dev pkg-config
```

Rust stable, edition 2021, MSRV 1.75. CI runs `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings` and `cargo test` on Windows, macOS
and Linux. Tests that use the real network or write to the real OS keychain are
`#[ignore]`d and opt-in, so CI never spends tokens or mutates a developer machine.

## Documentation

```text
docs/HOST_INTEGRATION.md   host integration guide (Rust, credentials, proxy, C ABI, Python)
docs/MODEL_DATA.md         model data pipeline, evidence chain, license gate
docs/CONTRACT_REVIEW.md    contract reference: which code implements each contract
docs/architecture/         V0 architecture and development plan
```

The design documents are written in Chinese.

## License

Dual-licensed under **MIT OR Apache-2.0**, at your option - see `LICENSE-MIT`
and `LICENSE-APACHE`. Distribution inside proprietary host software is permitted.

Bundled model data is generated only from redistributable sources (currently
MIT-licensed upstreams). Sources whose terms do not allow redistribution stay
build-time references and never enter the shipped catalog.
