"""补一个显示名择优的回归测试。"""
import io

P = r"C:\个人文件\API\model-runtime\runtime-data\src\pipeline.rs"
s = io.open(P, encoding="utf-8").read()

marker = "    #[test]\n    fn canonical_ids_are_unique_after_merge() {"
test = '''    #[test]
    fn display_name_prefers_the_cleanest_candidate() {
        // 实测上游名字带 provider 后缀与括号注释；取最长会把噪音挑给用户
        let mut a = third_party("models_dev", "p/deepseek-r1");
        a.display_name = Some("Pro/deepseek-ai/DeepSeek-R1".into());
        let mut b = third_party("litellm", "p/deepseek-r1");
        b.display_name = Some("DeepSeek R1".into());
        let mut c = third_party("models_dev", "q/deepseek-r1");
        c.display_name = Some("DeepSeek R1 (Vertex AI (OpenAI-compatible))".into());
        let out = build(vec![a, b, c], &licenses_all(), 0);
        assert_eq!(out.catalog.entries[0].display_name, "DeepSeek R1");
    }

'''
if "display_name_prefers_the_cleanest" not in s:
    assert marker in s, "marker not found"
    s = s.replace(marker, test + marker, 1)
    io.open(P, "w", encoding="utf-8", newline="\n").write(s)
    print("test inserted")
else:
    print("already present")
