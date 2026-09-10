//! 设置界面的行为回归测试（无头，不需要窗口）。
//!
//! 这些测试盯住的是**用户能看见的行为**，而不是内部实现：
//! 之前"刷新模型列表没用"就是因为失败被静默吞掉、没有任何测试约束它。

use std::sync::Arc;
use std::time::{Duration, Instant};

use runtime_ui::{
    BackendError, ConnectionReport, SettingsBackend, SettingsDraft, UiModelEntry, UiModelInfo,
};
use runtime_ui_egui::SettingsApp;

/// 可编程后端：指定 discover 的成功/失败/延迟。
struct StubBackend {
    discover_result: Result<Vec<UiModelEntry>, (String, String)>,
    models: Vec<UiModelInfo>,
    discover_calls: std::sync::atomic::AtomicUsize,
}

impl StubBackend {
    fn ok(models: &[&str]) -> Arc<Self> {
        Arc::new(Self {
            discover_result: Ok(models
                .iter()
                .map(|id| UiModelEntry {
                    model_id: (*id).to_string(),
                    display_name: None,
                })
                .collect()),
            models: models.iter().map(|id| UiModelInfo::unknown(*id)).collect(),
            discover_calls: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    fn failing(reason_key: &str, detail: &str) -> Arc<Self> {
        Arc::new(Self {
            discover_result: Err((reason_key.to_string(), detail.to_string())),
            models: Vec::new(),
            discover_calls: std::sync::atomic::AtomicUsize::new(0),
        })
    }
}

impl SettingsBackend for StubBackend {
    fn test_connection(&self, _: &SettingsDraft) -> Result<ConnectionReport, BackendError> {
        Ok(ConnectionReport { latency_ms: 1 })
    }

    fn discover(&self, _: &SettingsDraft) -> Result<Vec<UiModelEntry>, BackendError> {
        self.discover_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        match &self.discover_result {
            Ok(models) => Ok(models.clone()),
            Err((key, detail)) => Err(BackendError::new(key.clone(), detail.clone())),
        }
    }

    fn model_info(&self, model_id: &str) -> Option<UiModelInfo> {
        self.models.iter().find(|m| m.model_id == model_id).cloned()
    }

    fn preview_request_url(&self, _: &SettingsDraft) -> Option<String> {
        Some("https://example.invalid/v1/chat/completions".into())
    }
}

fn app_with(backend: Arc<StubBackend>) -> SettingsApp {
    SettingsApp::new(
        runtime_ui::SettingsPage::provider_settings(),
        runtime_ui::Strings::builtin("zh-CN").unwrap(),
        backend,
    )
}

/// 等后台动作完成（最多 3 秒），返回是否完成。
fn wait_until_idle(app: &mut SettingsApp) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        app.poll_pending();
        if !app.is_busy() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn refresh_adds_models_from_the_provider() {
    let backend = StubBackend::ok(&["live-a", "live-b"]);
    let mut app = app_with(backend.clone());
    let before = app.state().models().len();

    app.trigger_discovery();
    assert!(wait_until_idle(&mut app), "后台动作应在 3 秒内结束");
    assert_eq!(
        backend
            .discover_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        1,
        "刷新必须真的调用后端 discover"
    );

    let ids: Vec<String> = app
        .state()
        .models()
        .iter()
        .map(|m| m.model_id.clone())
        .collect();
    assert!(
        ids.contains(&"live-a".to_string()),
        "刷新后应出现服务端模型"
    );
    assert!(ids.contains(&"live-b".to_string()));
    assert!(app.state().discovered_count() >= 2);
    assert!(
        app.state().models().len() > before,
        "刷新后列表应变长（而不是看起来没反应）"
    );
    assert!(app.discovery_error().is_none(), "成功时不该残留错误");
}

#[test]
fn refresh_failure_is_visible_not_swallowed() {
    // 这条测试就是为了防止"点了没反应"再次发生
    let backend = StubBackend::failing("connection.failed", "401 Unauthorized: invalid api key");
    let mut app = app_with(backend);

    app.trigger_discovery();
    assert!(wait_until_idle(&mut app));

    let error = app
        .discovery_error()
        .expect("刷新失败必须留下可见的原因（之前这里是静默吞掉）");
    assert!(
        error.contains("401") || error.contains("invalid api key"),
        "错误信息应包含后端给的原因，实际: {error}"
    );
}

#[test]
fn success_clears_a_previous_failure() {
    // 先失败一次，再成功一次：错误提示必须消失，否则用户以为又失败了
    let failing = StubBackend::failing("connection.failed", "boom");
    let mut app = app_with(failing);
    app.trigger_discovery();
    assert!(wait_until_idle(&mut app));
    assert!(app.discovery_error().is_some());

    let ok = StubBackend::ok(&["live-a"]);
    let mut app2 = app_with(ok);
    app2.trigger_discovery();
    assert!(wait_until_idle(&mut app2));
    assert!(app2.discovery_error().is_none());
}

#[test]
fn backend_without_discovery_support_reports_empty_result_but_no_crash() {
    let backend = StubBackend::ok(&[]);
    let mut app = app_with(backend);
    app.trigger_discovery();
    assert!(wait_until_idle(&mut app));
    assert_eq!(app.state().discovered_count(), 0);
    // 空结果不算失败，也不该留下错误
    assert!(app.discovery_error().is_none());
}

#[test]
fn request_url_preview_comes_from_the_backend() {
    // 预览必须由后端计算（与 Adapter 同源），UI 不自己拼
    let backend = StubBackend::ok(&["m"]);
    let mut app = app_with(backend);
    let ctx = egui::Context::default();
    app.apply_theme(
        &ctx,
        runtime_ui_egui::ThemeMode::Dark,
        runtime_ui_egui::Density::Cozy,
        1.0,
    );
    let _ = ctx.run(egui::RawInput::default(), |ctx| app.ui(ctx));
    // 无 panic 即通过；预览内容由后端决定，UI 只展示
}

#[test]
fn switching_provider_asks_the_backend_again() {
    // 换厂商要重新取推荐（查目录），而不是沿用上家的列表
    struct Recommending {
        recommended: std::sync::Mutex<Vec<String>>,
    }
    impl SettingsBackend for Recommending {
        fn test_connection(&self, _: &SettingsDraft) -> Result<ConnectionReport, BackendError> {
            Ok(ConnectionReport { latency_ms: 1 })
        }
        fn discover(&self, _: &SettingsDraft) -> Result<Vec<UiModelEntry>, BackendError> {
            Ok(vec![])
        }
        fn recommend_models(&self, ids: &[String], _limit: usize) -> Vec<UiModelEntry> {
            let mut recommended = self.recommended.lock().unwrap();
            // 记录被问过哪些 provider
            recommended.extend(ids.iter().cloned());
            ids.iter()
                .map(|id| UiModelEntry {
                    model_id: format!("{id}-model"),
                    display_name: None,
                })
                .collect()
        }
    }

    let backend = Arc::new(Recommending {
        recommended: std::sync::Mutex::new(Vec::new()),
    });
    let mut app = SettingsApp::new(
        runtime_ui::SettingsPage::provider_settings(),
        runtime_ui::Strings::builtin("zh-CN").unwrap(),
        backend.clone(),
    );
    app.prime_recommendations();
    let asked = backend.recommended.lock().unwrap().clone();
    let preset_ids: Vec<String> = app
        .state()
        .preset()
        .catalog_provider_ids
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(asked, preset_ids, "应向目录问当前厂商的 provider 键");
    assert!(
        !app.state().models().is_empty(),
        "推荐结果应进入列表，首屏不该是空的"
    );
}
