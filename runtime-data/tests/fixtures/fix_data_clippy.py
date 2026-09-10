"""修正 runtime-data 的 clippy 报错。"""
import io

# 1) pipeline.rs: 未使用的导入
P = r"C:\个人文件\API\model-runtime\runtime-data\src\pipeline.rs"
s = io.open(P, encoding="utf-8").read()
s = s.replace("    use crate::record::{RawCapabilities, RawModelRecord};", "    use crate::record::RawModelRecord;")
io.open(P, "w", encoding="utf-8", newline="\n").write(s)
print("pipeline: import fixed")

# 2) store.rs: 文档列表缩进 + 测试里的 clone-to-slice
P2 = r"C:\个人文件\API\model-runtime\runtime-data\src\store.rs"
s = io.open(P2, encoding="utf-8").read()
s = s.replace(
    """//! **存储选型说明**：逻辑表结构与规格 §5 一致，但物理形态是
//! 「一表一 JSON 文件 + 原子写」，而不是 SQLite。理由：
//! - 宿主体积预算是硬约束，捆绑 SQLite 引擎（C 代码）会显著增大产物
//! - 本地数据量级是**每个用户几十条记录**，不是百万行，索引没有价值
//! - 无 C 依赖 → 三平台构建一致，无交叉编译麻烦""",
    """//! **存储选型说明**：逻辑表结构与规格 §5 一致，但物理形态是
//! 「一表一 JSON 文件 + 原子写」，而不是 SQLite。理由：
//!
//! - 宿主体积预算是硬约束，捆绑 SQLite 引擎（C 代码）会显著增大产物
//! - 本地数据量级是**每个用户几十条记录**，不是百万行，索引没有价值
//! - 无 C 依赖 → 三平台构建一致，无交叉编译麻烦""",
)
s = s.replace(
    "db.upsert_profiles(&[p1.clone()], &[], 100)",
    "db.upsert_profiles(std::slice::from_ref(&p1), &[], 100)",
)
s = s.replace(
    "db.upsert_profiles(&[p1.clone()], &[], 200)",
    "db.upsert_profiles(std::slice::from_ref(&p1), &[], 200)",
)
io.open(P2, "w", encoding="utf-8", newline="\n").write(s)
print("store: fixed")
