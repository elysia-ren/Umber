"""修：密钥必须显式传给后端（之前 to_draft 不含密钥，导致请求永远没带 key）。"""
import io

# ---------- 1) backend.rs：test_connection / discover 显式接收本次会话密钥 ----------
P_BE = r"C:\个人文件\API\model-runtime\runtime-ui\src\backend.rs"
b = io.open(P_BE, encoding="utf-8").read()

old = """pub trait SettingsBackend: Send + Sync {
    /// 测试连接。语义必须是 Passive（§17.1）：不得发送生成请求。
    fn test_connection(&self, draft: &SettingsDraft) -> Result<ConnectionReport, BackendError>;

    /// 模型发现（GET /models；失败可手动回退，§36）。
    fn discover(&self, draft: &SettingsDraft) -> Result<Vec<UiModelEntry>, BackendError>;"""
new = """pub trait SettingsBackend: Send + Sync {
    /// 测试连接。语义必须是 Passive（§17.1）：不得发送生成请求。
    ///
    /// `api_key` 是**本次会话**用户刚输入的密钥（`None` = 沿用已保存的）。
    /// 它与 `draft` 分开传递，因为：
    /// - `draft` 会被序列化（配置落盘），绝不能含密钥（§32）
    /// - 密钥只在内存里活一次请求，用完即弃，不进任何持久化结构
    fn test_connection(
        &self,
        draft: &SettingsDraft,
        api_key: Option<&str>,
    ) -> Result<ConnectionReport, BackendError>;

    /// 模型发现（GET /models；失败可手动回退，§36）。`api_key` 同上。
    fn discover(
        &self,
        draft: &SettingsDraft,
        api_key: Option<&str>,
    ) -> Result<Vec<UiModelEntry>, BackendError>;"""
assert old in b, "trait methods not found"
b = b.replace(old, new)

# 测试替身同步
b = b.replace(
    """        struct Minimal;
        impl SettingsBackend for Minimal {
            fn test_connection(
                &self,
                _: &SettingsDraft,
            ) -> Result<ConnectionReport, BackendError> {
                Ok(ConnectionReport { latency_ms: 1 })
            }
            fn discover(&self, _: &SettingsDraft) -> Result<Vec<UiModelEntry>, BackendError> {
                Ok(vec![])
            }
        }""",
    """        struct Minimal;
        impl SettingsBackend for Minimal {
            fn test_connection(
                &self,
                _: &SettingsDraft,
                _: Option<&str>,
            ) -> Result<ConnectionReport, BackendError> {
                Ok(ConnectionReport { latency_ms: 1 })
            }
            fn discover(
                &self,
                _: &SettingsDraft,
                _: Option<&str>,
            ) -> Result<Vec<UiModelEntry>, BackendError> {
                Ok(vec![])
            }
        }""",
)
io.open(P_BE, "w", encoding="utf-8", newline="\n").write(b)
print("backend.rs patched")

# ---------- 2) app.rs：调用处传上密钥 ----------
P_APP = r"C:\个人文件\API\model-runtime\runtime-ui-egui\src\app.rs"
a = io.open(P_APP, encoding="utf-8").read()
a = a.replace(
    """                let _ = tx.send(JobResult::Connection(backend.test_connection(&draft)));""",
    """                let _ = tx.send(JobResult::Connection(
                    backend.test_connection(&draft, session_key.as_deref()),
                ));""",
)
a = a.replace(
    """                let _ = tx.send(JobResult::Discovery(backend.discover(&draft)));""",
    """                let _ = tx.send(JobResult::Discovery(
                    backend.discover(&draft, session_key.as_deref()),
                ));""",
)
# 两个 start_* 里构造 session_key
a = a.replace(
    """    fn start_connection_test(&mut self) {
        self.connection = ConnectionTestState::Testing;
        let backend = self.backend.clone();
        let draft = self.state.to_draft();""",
    """    fn start_connection_test(&mut self) {
        self.connection = ConnectionTestState::Testing;
        let backend = self.backend.clone();
        let draft = self.state.to_draft();
        let session_key = self.session_key();""",
)
a = a.replace(
    """    fn start_discovery(&mut self) {
        let backend = self.backend.clone();
        let draft = self.state.to_draft();""",
    """    fn start_discovery(&mut self) {
        let backend = self.backend.clone();
        let draft = self.state.to_draft();
        let session_key = self.session_key();""",
)
# session_key 辅助
a = a.replace(
    """    /// 上次保存的状态（测试用）。""",
    """    /// 本次会话用户输入的密钥（空白视为未输入 → 让后端沿用已保存的）。
    fn session_key(&self) -> Option<String> {
        let typed = self.state.api_key().trim();
        (!typed.is_empty()).then(|| typed.to_string())
    }

    /// 上次保存的状态（测试用）。""",
)
io.open(P_APP, "w", encoding="utf-8", newline="\n").write(a)
print("app.rs patched")

# ---------- 3) demo：用显式传入的密钥 ----------
P_MAIN = r"C:\个人文件\API\model-runtime\runtime-ui-egui\src\main.rs"
m = io.open(P_MAIN, encoding="utf-8").read()
m = m.replace(
    """    /// 本次会话的凭据：优先用用户刚输入的，否则回落到已保存的（§32）。
    ///
    /// 只读回内存供本次调用使用，密钥不进入任何配置文件。
    fn credentials_for(&self, draft: &SettingsDraft) -> InMemoryCredentialStore {
        let store = InMemoryCredentialStore::new();
        let provided = draft
            .values
            .get("api_key")
            .filter(|k| !k.trim().is_empty())
            .cloned();
        let key = provided.or_else(|| {""",
    """    /// 本次会话的凭据：优先用用户刚输入的 `api_key`，否则回落到已保存的（§32）。
    ///
    /// 只读回内存供本次调用使用，密钥不进入任何配置文件。
    fn credentials_for(&self, api_key: Option<&str>) -> InMemoryCredentialStore {
        let store = InMemoryCredentialStore::new();
        let provided = api_key
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(str::to_string);
        let key = provided.or_else(|| {""",
)
m = m.replace(
    """    fn test_connection(&self, draft: &SettingsDraft) -> Result<ConnectionReport, BackendError> {
        let endpoint = Self::endpoint_of(draft)?;
        let credentials = self.credentials_for(draft);""",
    """    fn test_connection(
        &self,
        draft: &SettingsDraft,
        api_key: Option<&str>,
    ) -> Result<ConnectionReport, BackendError> {
        let endpoint = Self::endpoint_of(draft)?;
        // 关键：用户刚输入的密钥必须送到这里，否则请求没带 key，
        // 只会拿到 provider 的 "Authentication Fails"
        let credentials = self.credentials_for(api_key);""",
)
m = m.replace(
    """    fn discover(&self, draft: &SettingsDraft) -> Result<Vec<UiModelEntry>, BackendError> {
        let endpoint = Self::endpoint_of(draft)?;
        let credentials = self.credentials_for(draft);""",
    """    fn discover(
        &self,
        draft: &SettingsDraft,
        api_key: Option<&str>,
    ) -> Result<Vec<UiModelEntry>, BackendError> {
        let endpoint = Self::endpoint_of(draft)?;
        let credentials = self.credentials_for(api_key);""",
)
io.open(P_MAIN, "w", encoding="utf-8", newline="\n").write(m)
print("main.rs patched")
