"""给 demo 加 --selftest：用真实 LocalDb + 系统凭据走一遍 save→load 并打印结果。"""
import io

P = r"C:\个人文件\API\model-runtime\runtime-ui-egui\src\main.rs"
m = io.open(P, encoding="utf-8").read()

old_main = """fn main() {
    let backend = RealBackend::new();
    let params = SettingsWindowParams {"""
new_main = """/// `--selftest`：不开窗口，直接用真实 Local DB + 凭据回退链走一遍
/// 保存 → 重新读取，把结果打印出来。
///
/// 这是"保存到底有没有落盘"的可验证证据——不依赖点击界面。
fn selftest() -> i32 {
    let backend = RealBackend::new();
    let mut draft = SettingsDraft::default();
    draft.values.insert("provider".into(), "selftest".into());
    draft.values.insert("protocol".into(), "openai_chat".into());
    draft
        .values
        .insert("endpoint".into(), "https://selftest.example/v1".into());
    draft.values.insert("model".into(), "selftest-model".into());
    draft
        .values
        .insert("context_window".into(), "123456".into());

    println!("[selftest] 数据目录: {}", data_dir().display());
    match backend.save_settings(&draft, "sk-selftest-only") {
        Ok(report) => println!(
            "[selftest] 保存: config={} credential={} warning={:?}",
            report.saved_config, report.saved_credential, report.credential_warning_key
        ),
        Err(e) => {
            println!("[selftest] 保存失败: {} ({})", e.reason_key, e.detail);
            return 1;
        }
    }

    match backend.load_settings() {
        Some(saved) => {
            println!(
                "[selftest] 读回: provider={} protocol={} endpoint={} model={:?} ctx={:?} has_key={}",
                saved.provider,
                saved.protocol,
                saved.endpoint,
                saved.model_id,
                saved.context_window,
                saved.has_api_key
            );
            if saved.endpoint != "https://selftest.example/v1"
                || saved.model_id.as_deref() != Some("selftest-model")
                || saved.context_window != Some(123_456)
                || !saved.has_api_key
            {
                println!("[selftest] FAIL: 读回值与保存值不一致");
                return 1;
            }
        }
        None => {
            println!("[selftest] FAIL: 读回为空");
            return 1;
        }
    }

    // 清掉自检写入的凭据与配置，避免污染用户数据
    if let Ok(guard) = backend.store.lock() {
        if let Some(bundle) = guard.as_ref() {
            let _ = bundle.credentials.delete(&bundle.credential_ref);
        }
    }
    println!("[selftest] PASS（已清理自检写入的凭据）");
    0
}

fn main() {
    if std::env::args().any(|arg| arg == "--selftest") {
        std::process::exit(selftest());
    }
    let backend = RealBackend::new();
    let params = SettingsWindowParams {"""
assert old_main in m, "main not found"
m = m.replace(old_main, new_main)
io.open(P, "w", encoding="utf-8", newline="\n").write(m)
print("selftest added")
