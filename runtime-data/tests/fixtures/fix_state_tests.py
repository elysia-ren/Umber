"""把涉及旧行为的测试改成断言新契约（分类顺序、不再硬编码模型名）。"""
import io

P = r"C:\个人文件\API\model-runtime\runtime-ui\src\settings_state.rs"
s = io.open(P, encoding="utf-8").read()

# 1) 分类顺序：国内第一
old = """        assert_eq!(
            categories,
            vec![
                ProviderCategory::Official,
                ProviderCategory::China,
                ProviderCategory::Gateway,
                ProviderCategory::Local,
                ProviderCategory::Custom
            ]
        );"""
new = """        assert_eq!(
            categories,
            vec![
                ProviderCategory::China,
                ProviderCategory::Official,
                ProviderCategory::Gateway,
                ProviderCategory::Local,
                ProviderCategory::Custom
            ],
            "国内厂商必须排在最前"
        );"""
assert old in s, "category order test not found"
s = s.replace(old, new)

# 2) 选厂商不再注入模型名；改为断言"推荐由宿主写入"
old2 = """    #[test]
    fn selecting_a_provider_brings_out_protocol_endpoint_and_models() {
        let mut state = SettingsState::new();
        state.select_provider("dashscope");
        assert_eq!(state.endpoint(), "https://dashscope.aliyuncs.com/compatible-mode/v1");
        assert_eq!(state.protocol(), ProtocolKind::OpenAiChat);
        // 推荐模型直接进列表：用户不必先"获取"才能选
        assert!(
            !state.models().is_empty(),
            "选厂商后模型列表不应为空"
        );
        assert!(state.models().iter().any(|m| m.model_id == "qwen-plus"));
    }"""
new2 = """    #[test]
    fn selecting_a_provider_brings_out_protocol_and_endpoint_only() {
        let mut state = SettingsState::new();
        state.select_provider("dashscope");
        assert_eq!(
            state.endpoint(),
            "https://dashscope.aliyuncs.com/compatible-mode/v1"
        );
        assert_eq!(state.protocol(), ProtocolKind::OpenAiChat);
        // 关键契约：预置**不得**注入模型名。模型只能来自
        // ① 宿主按厂商查目录 ② 服务 /models 实时拉取 ③ 用户手填。
        assert!(
            state.models().is_empty(),
            "预置不该塞模型名（否则会出现一年前的型号）"
        );
    }

    #[test]
    fn recommendations_from_catalog_are_marked_and_replaceable() {
        let mut state = SettingsState::new();
        state.select_provider("deepseek");
        state.apply_recommendations(vec![
            ModelEntry::with_source("deepseek-v4-pro", None, ModelSource::Catalog),
            ModelEntry::with_source("deepseek-v4-flash", None, ModelSource::Catalog),
        ]);
        assert_eq!(state.models().len(), 2);
        assert!(state.models().iter().all(|m| m.source == ModelSource::Catalog));
        // 目录推荐被替换，不会累积
        state.apply_recommendations(vec![ModelEntry::with_source(
            "deepseek-v4-pro",
            None,
            ModelSource::Catalog,
        )]);
        assert_eq!(state.models().len(), 1);
    }

    #[test]
    fn discovery_replaces_only_its_own_entries() {
        let mut state = SettingsState::new();
        state.select_provider("deepseek");
        state.apply_recommendations(vec![ModelEntry::with_source(
            "catalog-model",
            None,
            ModelSource::Catalog,
        )]);
        state.add_model(ModelEntry::with_source(
            "manual-model",
            None,
            ModelSource::Manual,
        ));
        state.apply_discovery(vec![
            ModelEntry::with_source("live-a", None, ModelSource::Discovered),
            ModelEntry::with_source("live-b", None, ModelSource::Discovered),
        ]);
        let ids: Vec<&str> = state.models().iter().map(|m| m.model_id.as_str()).collect();
        // 实时清单进来，但目录推荐与手填都还在
        assert!(ids.contains(&"live-a"));
        assert!(ids.contains(&"catalog-model"), "目录推荐不该被实时结果清掉");
        assert!(ids.contains(&"manual-model"), "手填条目不该被清掉");

        // 再拉一次：只替换实时来源的条目
        state.apply_discovery(vec![ModelEntry::with_source(
            "live-c",
            None,
            ModelSource::Discovered,
        )]);
        let ids: Vec<&str> = state.models().iter().map(|m| m.model_id.as_str()).collect();
        assert!(ids.contains(&"live-c"));
        assert!(!ids.contains(&"live-a"), "上一轮实时结果应被替换");
        assert!(ids.contains(&"catalog-model"));
    }"""
assert old2 in s, "selecting test not found"
s = s.replace(old2, new2)

# 3) 切厂商：不再断言某家模型还在，只断言旧的一律清空
old3 = """    #[test]
    fn switching_provider_clears_previous_models() {
        let mut state = SettingsState::new();
        state.select_provider("openai");
        assert!(state.models().iter().any(|m| m.model_id == "gpt-4o-mini"));
        state.select_provider("anthropic");
        // 上家的模型不该留在列表里
        assert!(!state.models().iter().any(|m| m.model_id == "gpt-4o-mini"));
        assert!(state.models().iter().any(|m| m.model_id == "claude-sonnet-4-5"));
    }"""
new3 = """    #[test]
    fn switching_provider_clears_previous_models() {
        let mut state = SettingsState::new();
        state.select_provider("openai");
        state.apply_recommendations(vec![ModelEntry::with_source(
            "gpt-4o-mini",
            None,
            ModelSource::Catalog,
        )]);
        assert!(!state.models().is_empty());
        state.select_provider("anthropic");
        // 换厂商后上家的模型必须清空（协议/端点/凭据都变了）
        assert!(state.models().is_empty(), "换厂商应清空模型列表");
        assert_eq!(state.selected_model(), None);
    }"""
assert old3 in s, "switching test not found"
s = s.replace(old3, new3)

io.open(P, "w", encoding="utf-8", newline="\n").write(s)
print("tests updated")
