"""Universal Embedded Model Runtime — Python ctypes 绑定。

设计要点（对应总案 §50 的 ABI 规则）：

- **无第三方依赖**：只用标准库 `ctypes`，任何 Python 环境可用
- **所有权明确**：事件 JSON 由 `runtime_string_free` 释放，绑定内部用
  try/finally 保证不泄漏
- **拉取式流**：`next_event()` 阻塞至多 `timeout_ms`；`WOULD_BLOCK` 是
  正常状态而非错误（可继续拉取）
- **终结保证**：迭代器在收到终结事件后停止；宿主不需要判断"流是不是死了"
- **错误码**：负数是错误，统一转为 `UmerAbiError`

用法：

    from umer import Runtime

    with Runtime() as rt:
        stream = rt.open_stream(request_json)
        for event in stream:
            print(event.sequence, event.data["event"]["type"])
        print("partial:", stream.partial_text)

真实调用（0.2 起；未配置且未开 demo 时 open_stream 会抛 NOT_CONFIGURED）：

    with Runtime() as rt:
        rt.set_deployment({
            "id": "deepseek/official/openai_chat/deepseek-chat",
            "provider_id": "deepseek",
            "protocol": "openai_chat",          # openai_chat / openai_responses
            "endpoint_url": "https://api.deepseek.com/v1",   # / anthropic_messages / gemini
            "model_id": "deepseek-chat",
            "credential_ref": "deepseek/api_key",
        })
        rt.set_credential("deepseek/api_key", "sk-...")   # 只在内存，进程退出即丢
        rt.load_catalog("catalog.json")                   # 可选：离线模型知识
        with rt.stream({"model": "deepseek/official/openai_chat/deepseek-chat",
                        "messages": [{"role": "user",
                                      "content": [{"type": "text", "text": "你好"}]}]}) as stream:
            for event in stream:
                inner = event["data"]["event"]
                if inner["type"] == "text_delta":
                    print(inner["delta"], end="", flush=True)
"""

from __future__ import annotations

import ctypes
import json
import os
import sys
from typing import Iterator, Optional

# ---- ABI 常量（与 runtime-ffi/src/lib.rs 一致）----
ABI_MAJOR = 0
ABI_MINOR = 2

UMER_OK = 0
UMER_ERR_NULL_ARGUMENT = -1
UMER_ERR_ABI_MISMATCH = -2
UMER_ERR_OPEN_FAILED = -3
UMER_ERR_INTERNAL = -4
UMER_ERR_NOT_CONFIGURED = -5
UMER_ERR_CATALOG = -6

UMER_EVENT = 1
UMER_CLOSED = 2
UMER_WOULD_BLOCK = 3

_ERROR_NAMES = {
    UMER_ERR_NULL_ARGUMENT: "NULL_ARGUMENT",
    UMER_ERR_ABI_MISMATCH: "ABI_MISMATCH",
    UMER_ERR_OPEN_FAILED: "OPEN_FAILED",
    UMER_ERR_INTERNAL: "INTERNAL",
    UMER_ERR_NOT_CONFIGURED: "NOT_CONFIGURED",
    UMER_ERR_CATALOG: "CATALOG",
}

TERMINAL_EVENT_TYPES = {"completed", "failed", "cancelled"}


class UmerAbiError(RuntimeError):
    """ABI 返回负错误码。"""

    def __init__(self, code: int, context: str = ""):
        name = _ERROR_NAMES.get(code, f"UNKNOWN({code})")
        super().__init__(f"UMER ABI error {name}" + (f" during {context}" if context else ""))
        self.code = code


class UmerEvent(ctypes.Structure):
    # json 必须是 c_void_p 而非 c_char_p：c_char_p 字段在访问时会被 ctypes
    # 自动解引用成 Python bytes，再交给 runtime_string_free 就会把 Python
    # 自己的缓冲区递去 free（实测为堆损坏 0xC0000374）。保持原始指针。
    _fields_ = [
        ("sequence", ctypes.c_uint64),
        ("json", ctypes.c_void_p),
        ("json_len", ctypes.c_size_t),
    ]


def _take_json(lib, out: "UmerEvent") -> str:
    """读出 ABI 交付的 JSON 并释放所有权（规则：谁分配谁释放，§50.2）。"""
    try:
        raw = ctypes.string_at(out.json, out.json_len)
    finally:
        lib.runtime_string_free(out.json)
    return raw.decode("utf-8")


def _library_candidates() -> list[str]:
    """按布局查找动态库。

    支持两种布局：

    - 源码仓库：``<repo>/runtime-ffi/bindings/python/umer.py``
      → 库在 ``<repo>/target/{debug,release}``
    - 集成包：``<pkg>/bindings/python/umer.py``
      → 库在 ``<pkg>/lib``

    生产环境建议显式传路径（``Runtime(path)``），别依赖自动查找。
    """
    names = ["runtime_ffi.dll", "libruntime_ffi.so", "libruntime_ffi.dylib"]
    here = os.path.dirname(os.path.abspath(__file__))
    roots = [
        # 集成包布局：bindings/python -> <pkg>/lib
        os.path.join(here, "..", "..", "lib"),
        os.path.join(here, "..", "lib"),
        # 源码仓库布局：runtime-ffi/bindings/python -> <repo>/target/*
        os.path.join(here, "..", "..", "..", "target", "release"),
        os.path.join(here, "..", "..", "..", "target", "debug"),
        os.path.join(here, "..", "..", "examples", "bin"),
        # 同目录（最常见的手动放置）
        here,
    ]
    return [
        os.path.normpath(os.path.join(root, name))
        for root in roots
        for name in names
    ]


def load_library(path: Optional[str] = None):
    """加载 cdylib。显式路径优先，其次按常见构建输出位置查找。"""
    if path:
        return ctypes.CDLL(path)
    for candidate in _library_candidates():
        if os.path.exists(candidate):
            return ctypes.CDLL(candidate)
    raise FileNotFoundError(
        "runtime_ffi shared library not found. Looked in: "
        + ", ".join(_library_candidates())
        + ". Pass an explicit path, e.g. Runtime(r'<pkg>/lib/runtime_ffi.dll')."
    )


class Stream:
    """一次 Invocation 的拉取式事件流。"""

    def __init__(self, lib, handle, invocation_label: str = ""):
        self._lib = lib
        self._handle = handle
        self._closed = False
        self._label = invocation_label
        self.events: list[dict] = []
        self.partial_text: str = ""

    # ---- 拉取 ----
    def next_event(self, timeout_ms: int = 2000) -> Optional[dict]:
        """拉取一个事件；流已终结返回 None。"""
        if self._closed:
            return None
        out = UmerEvent()
        status = self._lib.runtime_stream_next(self._handle, timeout_ms, ctypes.byref(out))
        while status == UMER_WOULD_BLOCK:
            status = self._lib.runtime_stream_next(self._handle, timeout_ms, ctypes.byref(out))
        if status == UMER_CLOSED:
            self._closed = True
            return None
        if status != UMER_EVENT:
            raise UmerAbiError(status, "runtime_stream_next")

        payload = json.loads(_take_json(self._lib, out))
        event = {"sequence": out.sequence, "data": payload}
        self.events.append(event)

        inner = payload.get("event", {})
        kind = inner.get("type", "")
        if inner.get("type") == "text_delta":
            self.partial_text += inner.get("delta", "")
        if kind in TERMINAL_EVENT_TYPES:
            self._closed = True
            if kind == "completed":
                response = inner.get("response", {})
                self.partial_text = "".join(
                    block.get("text", "")
                    for block in response.get("content", [])
                    if block.get("type") == "text"
                )
            elif kind == "failed":
                error = inner.get("error", {})
                raise UmerRuntimeFailure(error)
        return event

    def __iter__(self) -> Iterator[dict]:
        while True:
            event = self.next_event()
            if event is None:
                return
            yield event

    # ---- 取消 ----
    def cancel(self) -> None:
        code = self._lib.runtime_stream_cancel(self._handle)
        if code != UMER_OK:
            raise UmerAbiError(code, "runtime_stream_cancel")

    def close(self) -> None:
        if self._handle:
            self._lib.runtime_stream_close(self._handle)
            self._handle = None
            self._closed = True

    def __enter__(self) -> "Stream":
        return self

    def __exit__(self, *exc) -> None:
        self.close()


class UmerRuntimeFailure(RuntimeError):
    """流以 Failed 终结：Canonical 错误已映射为异常（§28）。"""

    def __init__(self, error: dict):
        kind = error.get("type", "unknown")
        message = error.get("message") or error.get("detail", {}).get("message", "")
        super().__init__(f"{kind}: {message}")
        self.error = error
        self.kind = kind
        # retryable 语义与 Rust 侧一致（§29）；此处仅暴露原始错误供宿主判断
        self.retryable = kind in {"rate_limited", "overloaded", "timeout", "network_error"}


class Runtime:
    """Runtime 句柄。"""

    def __init__(self, library_path: Optional[str] = None, expected_major: int = ABI_MAJOR):
        self._lib = load_library(library_path)
        self._bind()
        version = self._lib.runtime_abi_version()
        major = version >> 16
        if major != expected_major:
            raise UmerAbiError(UMER_ERR_ABI_MISMATCH, f"abi {major} != expected {expected_major}")
        self.abi_version = (major, version & 0xFFFF)
        self._handle = self._lib.runtime_init()
        if not self._handle:
            raise UmerAbiError(UMER_ERR_INTERNAL, "runtime_init")

    def _bind(self) -> None:
        lib = self._lib
        lib.runtime_abi_version.restype = ctypes.c_uint32
        lib.runtime_init.restype = ctypes.c_void_p
        lib.runtime_shutdown.argtypes = [ctypes.c_void_p]
        lib.runtime_stream_open.argtypes = [
            ctypes.c_void_p,
            ctypes.c_char_p,
            ctypes.c_size_t,
            ctypes.POINTER(ctypes.c_void_p),
        ]
        lib.runtime_stream_open.restype = ctypes.c_int32
        lib.runtime_stream_next.argtypes = [
            ctypes.c_void_p,
            ctypes.c_uint32,
            ctypes.POINTER(UmerEvent),
        ]
        lib.runtime_stream_next.restype = ctypes.c_int32
        lib.runtime_stream_cancel.argtypes = [ctypes.c_void_p]
        lib.runtime_stream_cancel.restype = ctypes.c_int32
        lib.runtime_stream_close.argtypes = [ctypes.c_void_p]
        lib.runtime_set_demo.argtypes = [ctypes.c_void_p, ctypes.c_int32]
        lib.runtime_set_demo.restype = ctypes.c_int32
        lib.runtime_set_deployment.argtypes = [
            ctypes.c_void_p,
            ctypes.c_char_p,
            ctypes.c_size_t,
        ]
        lib.runtime_set_deployment.restype = ctypes.c_int32
        lib.runtime_set_credential.argtypes = [
            ctypes.c_void_p,
            ctypes.c_char_p,
            ctypes.c_char_p,
        ]
        lib.runtime_set_credential.restype = ctypes.c_int32
        lib.runtime_load_catalog.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
        lib.runtime_load_catalog.restype = ctypes.c_int32
        lib.runtime_status.argtypes = [ctypes.c_void_p, ctypes.POINTER(UmerEvent)]
        lib.runtime_status.restype = ctypes.c_int32
        lib.runtime_set_demo.argtypes = [ctypes.c_void_p, ctypes.c_int32]
        lib.runtime_set_demo.restype = ctypes.c_int32
        lib.runtime_set_deployment.argtypes = [
            ctypes.c_void_p,
            ctypes.c_char_p,
            ctypes.c_size_t,
        ]
        lib.runtime_set_deployment.restype = ctypes.c_int32
        lib.runtime_set_credential.argtypes = [
            ctypes.c_void_p,
            ctypes.c_char_p,
            ctypes.c_char_p,
        ]
        lib.runtime_set_credential.restype = ctypes.c_int32
        lib.runtime_load_catalog.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
        lib.runtime_load_catalog.restype = ctypes.c_int32
        lib.runtime_status.argtypes = [ctypes.c_void_p, ctypes.POINTER(UmerEvent)]
        lib.runtime_status.restype = ctypes.c_int32
        # 用 c_void_p 而非 c_char_p：后者会把 Python bytes 的缓冲区指针传过去
        lib.runtime_string_free.argtypes = [ctypes.c_void_p]

    # ---- 配置（真实链路，0.2 起）----

    def set_demo(self, enabled: bool = True) -> None:
        """开启内置 demo 事件源。假数据，仅用于验证 ABI 形态，默认关闭。"""
        code = self._lib.runtime_set_demo(self._handle, 1 if enabled else 0)
        if code != UMER_OK:
            raise UmerAbiError(code, "runtime_set_demo")

    def set_deployment(self, config: dict) -> None:
        """注册（或覆盖）一个 Deployment。

        必填键：id / protocol / endpoint_url / model_id；
        可选：provider_id（默认 custom）、endpoint_id、credential_ref。
        请求里的 model 必须等于这里的 id。
        """
        payload = json.dumps(config, ensure_ascii=False).encode("utf-8")
        code = self._lib.runtime_set_deployment(self._handle, payload, len(payload))
        if code != UMER_OK:
            raise UmerAbiError(code, "runtime_set_deployment")

    def set_credential(self, reference: str, secret: str) -> None:
        """写入一条凭据到 Runtime 内存存储。

        Runtime 不持久化密钥（进程退出即丢）。需要持久化的宿主应自己
        落到系统钥匙串，启动时再写入本函数。
        """
        code = self._lib.runtime_set_credential(
            self._handle, reference.encode("utf-8"), secret.encode("utf-8")
        )
        if code != UMER_OK:
            raise UmerAbiError(code, "runtime_set_credential")

    def load_catalog(self, path: str) -> None:
        """加载 Canonical Catalog（离线模型知识：能力 / 上下文 / 价格）。"""
        code = self._lib.runtime_load_catalog(self._handle, os.fsencode(path))
        if code != UMER_OK:
            raise UmerAbiError(code, "runtime_load_catalog")

    def status(self) -> dict:
        """运行时状态：已配置部署 / 目录条目 / demo 开关。"""
        out = UmerEvent()
        code = self._lib.runtime_status(self._handle, ctypes.byref(out))
        if code != UMER_EVENT:
            raise UmerAbiError(code, "runtime_status")
        return json.loads(_take_json(self._lib, out))

    def open_stream(self, request_json: str) -> Stream:
        payload = request_json.encode("utf-8")
        handle = ctypes.c_void_p()
        code = self._lib.runtime_stream_open(
            self._handle, payload, len(payload), ctypes.byref(handle)
        )
        if code != UMER_OK:
            raise UmerAbiError(code, "runtime_stream_open")
        return Stream(self._lib, handle)

    def stream(self, request: dict, timeout_ms: int = 2000) -> Stream:
        """便捷入口：直接传 Canonical GenerateRequest 的 dict。"""
        return self.open_stream(json.dumps(request, ensure_ascii=False))

    def __enter__(self) -> "Runtime":
        return self

    def __exit__(self, *exc) -> None:
        if self._handle:
            self._lib.runtime_shutdown(self._handle)
            self._handle = None


def main() -> int:
    """最小自检：开启 demo 流，拉取到终结事件。

    demo 是内置假事件源，只验证 ABI 形态（握手 / 拉取 / 所有权 / 终结保证）。
    真实调用见模块文档的"真实调用"示例。
    """
    request = {
        "model": "demo",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}],
    }
    with Runtime() as rt:
        print(f"[py] abi {rt.abi_version[0]}.{rt.abi_version[1]}")
        rt.set_demo()
        print(f"[py] status {rt.status()}")
        with rt.stream(request) as stream:
            terminals = 0
            for event in stream:
                inner = event["data"]["event"]
                print(f"[py] seq={event['sequence']} type={inner['type']}")
                if inner["type"] in TERMINAL_EVENT_TYPES:
                    terminals += 1
            assert terminals == 1, f"终结事件必须恰好一个，实际 {terminals}"
            print(f"[py] 事件数={len(stream.events)} 文本={stream.partial_text!r}")
    print("[py] PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
