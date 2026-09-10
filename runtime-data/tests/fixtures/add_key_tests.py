"""加连接测试的公开钩子 + 两条密钥传递的回归测试。"""
import io

# 1) app.rs：公开 trigger_connection_test
P_APP = r"C:\个人文件\API\model-runtime\runtime-ui-egui\src\app.rs"
a = io.open(P_APP, encoding="utf-8").read()
old = """    /// 触发一次"刷新模型列表"（走服务商 `/models`）。测试用入口。
    pub fn trigger_discovery(&mut self) {
        self.start_discovery();
    }"""
new = """    /// 触发一次"刷新模型列表"（走服务商 `/models`）。测试用入口。
    pub fn trigger_discovery(&mut self) {
        self.start_discovery();
    }

    /// 触发一次"测试连接"。测试用入口。
    pub fn trigger_connection_test(&mut self) {
        self.start_connection_test();
    }"""
if "pub fn trigger_connection_test" not in a:
    assert old in a, "trigger_discovery hook not found"
    a = a.replace(old, new)
    io.open(P_APP, "w", encoding="utf-8", newline="\n").write(a)
    print("app.rs: hook added")
else:
    print("app.rs: hook already present")

# 2) 回归测试
P_T = r"C:\个人文件\API\model-runtime\runtime-ui-egui\tests\refresh_behaviour.rs"
s = io.open(P_T, encoding="utf-8").read()

TESTS = '''#[test]
fn the_key_typed_in_the_ui_reaches_the_backend() {
    // 回归：曾经 to_draft() 不含密钥，用户填了 key 请求仍然不带凭据，
    // 于是永远拿到 provider 的 "Authentication Fails"。
    struct KeyCapturing {
        seen: std::sync::Mutex<Vec<Option<String>>>,
    }
    impl SettingsBackend for KeyCapturing {
        fn test_connection(
            &self,
            _: &SettingsDraft,
            api_key: Option<&str>,
        ) -> Result<ConnectionReport, BackendError> {
            self.seen.lock().unwrap().push(api_key.map(str::to_string));
            Ok(ConnectionReport { latency_ms: 1 })
        }
        fn discover(
            &self,
            _: &SettingsDraft,
            api_key: Option<&str>,
        ) -> Result<Vec<UiModelEntry>, BackendError> {
            self.seen.lock().unwrap().push(api_key.map(str::to_string));
            Ok(vec![UiModelEntry {
                model_id: "m".into(),
                display_name: None,
            }])
        }
    }

    let backend = Arc::new(KeyCapturing {
        seen: std::sync::Mutex::new(Vec::new()),
    });
    let mut app = SettingsApp::new(
        runtime_ui::SettingsPage::provider_settings(),
        runtime_ui::Strings::builtin("zh-CN").unwrap(),
        backend.clone(),
    );
    app.state_mut().select_provider("deepseek");
    app.state_mut().set_api_key("sk-TYPED-BY-USER");

    app.trigger_discovery();
    assert!(wait_until_idle(&mut app));
    app.trigger_connection_test();
    assert!(wait_until_idle(&mut app));

    let seen = backend.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 2, "刷新与测试连接都应调用后端");
    for key in &seen {
        assert_eq!(
            key.as_deref(),
            Some("sk-TYPED-BY-USER"),
            "界面里填的密钥必须送到后端，否则请求不带凭据"
        );
    }
}

#[test]
fn an_empty_key_field_means_use_the_saved_one() {
    // 用户没重新输入时应传 None（让后端沿用已保存的密钥），
    // 而不是传空串把凭据清掉
    struct KeyCapturing(std::sync::Mutex<Vec<Option<String>>>);
    impl SettingsBackend for KeyCapturing {
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
            api_key: Option<&str>,
        ) -> Result<Vec<UiModelEntry>, BackendError> {
            self.0.lock().unwrap().push(api_key.map(str::to_string));
            Ok(vec![])
        }
    }
    let backend = Arc::new(KeyCapturing(std::sync::Mutex::new(Vec::new())));
    let mut app = SettingsApp::new(
        runtime_ui::SettingsPage::provider_settings(),
        runtime_ui::Strings::builtin("zh-CN").unwrap(),
        backend.clone(),
    );
    app.state_mut().select_provider("deepseek");
    // 刻意只填空格
    app.state_mut().set_api_key("   ");
    app.trigger_discovery();
    assert!(wait_until_idle(&mut app));
    let seen = backend.0.lock().unwrap().clone();
    assert_eq!(seen, vec![None], "空白输入应传 None，而不是空串");
}

'''

marker = "#[test]\nfn request_url_preview_comes_from_the_backend() {"
if "the_key_typed_in_the_ui_reaches_the_backend" not in s:
    assert marker in s, "marker not found"
    s = s.replace(marker, TESTS + marker, 1)
    io.open(P_T, "w", encoding="utf-8", newline="\n").write(s)
    print("tests inserted")
else:
    print("tests already present")
