"""同步所有 SettingsBackend 实现的方法签名（加 api_key 参数）。"""
import io
import re

TARGETS = [
    r"C:\个人文件\API\model-runtime\runtime-ui-egui\src\lib.rs",
    r"C:\个人文件\API\model-runtime\runtime-ui\src\backend.rs",
    r"C:\个人文件\API\model-runtime\runtime-ui-egui\tests\refresh_behaviour.rs",
    r"C:\个人文件\API\model-runtime\runtime-ui-egui\tests\save_behaviour.rs",
]

for path in TARGETS:
    s = io.open(path, encoding="utf-8").read()
    before = s
    # 旧签名 → 新签名（含参数名与多行形式）
    s = s.replace(
        "fn test_connection(&self, _: &SettingsDraft) -> Result<ConnectionReport, BackendError> {",
        "fn test_connection(\n            &self,\n            _: &SettingsDraft,\n            _: Option<&str>,\n        ) -> Result<ConnectionReport, BackendError> {",
    )
    s = s.replace(
        "fn discover(&self, _: &SettingsDraft) -> Result<Vec<UiModelEntry>, BackendError> {",
        "fn discover(\n            &self,\n            _: &SettingsDraft,\n            _: Option<&str>,\n        ) -> Result<Vec<UiModelEntry>, BackendError> {",
    )
    s = s.replace(
        "fn test_connection(&self, _: &SettingsDraft) -> Result<ConnectionReport, BackendError> {",
        "fn test_connection(\n        &self,\n        _: &SettingsDraft,\n        _: Option<&str>,\n    ) -> Result<ConnectionReport, BackendError> {",
    )
    s = s.replace(
        "fn discover(&self, _: &SettingsDraft) -> Result<Vec<UiModelEntry>, BackendError> {",
        "fn discover(\n        &self,\n        _: &SettingsDraft,\n        _: Option<&str>,\n    ) -> Result<Vec<UiModelEntry>, BackendError> {",
    )
    # 测试替身里的命名参数形式
    s = s.replace(
        "fn test_connection(&self, _: &SettingsDraft) -> Result<ConnectionReport, BackendError> {\n            Ok(ConnectionReport { latency_ms: 1 })\n        }",
        "fn test_connection(\n            &self,\n            _: &SettingsDraft,\n            _: Option<&str>,\n        ) -> Result<ConnectionReport, BackendError> {\n            Ok(ConnectionReport { latency_ms: 1 })\n        }",
    )
    if s != before:
        io.open(path, "w", encoding="utf-8", newline="\n").write(s)
        print("patched:", path.rsplit("\\", 1)[-1])
    else:
        print("no change:", path.rsplit("\\", 1)[-1])
