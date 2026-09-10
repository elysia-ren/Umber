"""为新的两栏设置界面补齐 i18n key（中英各一套）。"""
import io
import re

PATH = r"C:\个人文件\API\model-runtime\runtime-ui\src\strings.rs"

ZH = '''
    // ---- 两栏设置界面 ----
    ("providers.search", "搜索厂商…"),
    ("providers.none", "没有匹配的厂商"),
    ("providers.docs", "文档"),
    ("providers.keyless", "免密"),
    ("providers.request_to", "请求将发送到"),
    ("providers.models", "模型列表"),
    ("providers.refresh_models", "刷新模型列表"),
    ("category.official", "官方直连"),
    ("category.china", "国内厂商"),
    ("category.gateway", "聚合与中转"),
    ("category.local", "本地部署"),
    ("category.custom", "自定义"),
    ("connection.test", "测试连接"),
    ("model.data.absent", "目录中没有该模型的数据（仍可正常使用）"),
    ("model.pricing", "价格"),
    ("model.price.input", "输入"),
    ("model.price.output", "输出"),
    ("model.price.cached", "缓存"),
    ("model.context_window", "上下文窗口"),
    ("model.context_window.hint", "留空使用自动探测值"),
    ("model.context.probed", "自动探测"),
    ("model.context.catalog", "目录"),
    ("model.context.overridden", "覆盖"),
    ("model.context.effective", "生效"),
    ("model.context.conflict", "（与其他来源不一致）"),
    ("model.context.unknown", "尚无数据：未探测且目录中无该模型"),
    // 新增厂商
    ("provider.xai", "xAI Grok"),
    ("provider.mistral", "Mistral"),
    ("provider.groq", "Groq"),
    ("provider.perplexity", "Perplexity"),
    ("provider.zhipu", "智谱 GLM"),
    ("provider.zai", "Z.AI（智谱国际）"),
    ("provider.moonshot", "Kimi / Moonshot"),
    ("provider.dashscope", "阿里云百炼"),
    ("provider.qianfan", "百度智能云千帆"),
    ("provider.hunyuan", "腾讯混元"),
    ("provider.minimax", "MiniMax"),
    ("provider.stepfun", "阶跃星辰 StepFun"),
    ("provider.sensenova", "商汤日日新"),
    ("provider.ark", "火山方舟"),
    ("provider.siliconflow", "硅基流动 SiliconFlow"),
    ("provider.modelscope", "魔搭 ModelScope"),
    ("provider.openrouter", "OpenRouter"),
    ("provider.together", "Together AI"),
    ("provider.fireworks", "Fireworks AI"),
    ("provider.nvidia", "NVIDIA NIM"),
    ("provider.cerebras", "Cerebras"),
    ("provider.ollama", "Ollama（本地）"),
    ("provider.lmstudio", "LM Studio（本地）"),
    ("provider.vllm", "vLLM（本地）"),
    ("validation.key.empty_hint", "尚未填写密钥"),
'''

EN = '''
    ("providers.search", "Search providers…"),
    ("providers.none", "No matching provider"),
    ("providers.docs", "Docs"),
    ("providers.keyless", "No key needed"),
    ("providers.request_to", "Requests go to"),
    ("providers.models", "Models"),
    ("providers.refresh_models", "Refresh models"),
    ("category.official", "Official"),
    ("category.china", "China"),
    ("category.gateway", "Gateways"),
    ("category.local", "Local"),
    ("category.custom", "Custom"),
    ("connection.test", "Test connection"),
    ("model.data.absent", "No catalog data for this model (still usable)"),
    ("model.pricing", "Pricing"),
    ("model.price.input", "in"),
    ("model.price.output", "out"),
    ("model.price.cached", "cached"),
    ("model.context_window", "Context window"),
    ("model.context_window.hint", "Leave empty to use the probed value"),
    ("model.context.probed", "probed"),
    ("model.context.catalog", "catalog"),
    ("model.context.overridden", "override"),
    ("model.context.effective", "effective"),
    ("model.context.conflict", "(differs from other sources)"),
    ("model.context.unknown", "No data yet: not probed and not in the catalog"),
    ("provider.xai", "xAI Grok"),
    ("provider.mistral", "Mistral"),
    ("provider.groq", "Groq"),
    ("provider.perplexity", "Perplexity"),
    ("provider.zhipu", "Zhipu GLM"),
    ("provider.zai", "Z.AI"),
    ("provider.moonshot", "Kimi / Moonshot"),
    ("provider.dashscope", "Alibaba Bailian"),
    ("provider.qianfan", "Baidu Qianfan"),
    ("provider.hunyuan", "Tencent Hunyuan"),
    ("provider.minimax", "MiniMax"),
    ("provider.stepfun", "StepFun"),
    ("provider.sensenova", "SenseNova"),
    ("provider.ark", "Volcengine Ark"),
    ("provider.siliconflow", "SiliconFlow"),
    ("provider.modelscope", "ModelScope"),
    ("provider.openrouter", "OpenRouter"),
    ("provider.together", "Together AI"),
    ("provider.fireworks", "Fireworks AI"),
    ("provider.nvidia", "NVIDIA NIM"),
    ("provider.cerebras", "Cerebras"),
    ("provider.ollama", "Ollama (local)"),
    ("provider.lmstudio", "LM Studio (local)"),
    ("provider.vllm", "vLLM (local)"),
    ("validation.key.empty_hint", "No API key yet"),
'''


def insert(text, marker, block):
    idx = text.index(marker)
    return text[:idx] + block.lstrip("\n") + "\n" + text[idx:]


with io.open(PATH, encoding="utf-8") as f:
    src = f.read()

# 中文（第一个 ') ];' 之前的 zh 数组末尾）
zh_marker = '    ("model.pricing.notice", "价格仅供参考，不作为计费依据"),'
en_marker = '    ("model.pricing.notice", "Pricing is informational only and not a basis for billing"),'

if "providers.search" not in src:
    src = src.replace(zh_marker, zh_marker + "\n" + ZH.strip("\n"))
    src = src.replace(en_marker, en_marker + "\n" + EN.strip("\n"))
    with io.open(PATH, "w", encoding="utf-8", newline="\n") as f:
        f.write(src)
    print("inserted")
else:
    print("already present")
