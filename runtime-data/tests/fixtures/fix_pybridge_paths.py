"""让 Python 绑定在两种布局下都能找到动态库：源码仓库 与 集成包。"""
import io

P = r"C:\个人文件\API\model-runtime\runtime-ffi\bindings\python\umer.py"
s = io.open(P, encoding="utf-8").read()

old = '''def _library_candidates() -> list[str]:
    names = ["runtime_ffi.dll", "libruntime_ffi.so", "libruntime_ffi.dylib"]
    here = os.path.dirname(os.path.abspath(__file__))
    roots = [
        os.path.join(here, "..", "..", "..", "target", "debug"),
        os.path.join(here, "..", "..", "..", "target", "release"),
        os.path.join(here, "..", "..", "examples", "bin"),
        here,
    ]
    found: list[str] = []
    for root in roots:
        for name in names:
            found.append(os.path.normpath(os.path.join(root, name)))
    return found'''

new = '''def _library_candidates() -> list[str]:
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
    ]'''

assert old in s, "library candidate block not found"
s = s.replace(old, new)

s = s.replace(
    '''    raise FileNotFoundError(
        "runtime_ffi shared library not found; build it with "
        "`cargo build -p runtime-ffi` or pass an explicit path"
    )''',
    '''    raise FileNotFoundError(
        "runtime_ffi shared library not found. Looked in: "
        + ", ".join(_library_candidates())
        + ". Pass an explicit path, e.g. Runtime(r'<pkg>/lib/runtime_ffi.dll')."
    )''',
)

io.open(P, "w", encoding="utf-8", newline="\n").write(s)
print("umer.py candidates updated")
