# 第三方依赖、许可与维护状态

言台原生后端只固定使用下列直接依赖。正式构建必须使用 `Cargo.lock` 和 `--locked`；
六个目标的完整传递图由 `deny.toml` 约束为兼容许可和 crates.io 来源。

| 依赖 | 固定版本 | 用途 | 许可 |
| --- | --- | --- | --- |
| `winit` | 0.30.13 | Windows、macOS、Wayland、X11 窗口与输入事件 | Apache-2.0 |
| `softbuffer` | 0.4.8 | 无 GPU 要求的原生窗口 CPU 表面 | MIT OR Apache-2.0 |
| `tiny-skia` | 0.11.4 | 跨平台一致的二维栅格化 | BSD-3-Clause |
| `cosmic-text` | 0.19.0 | 字体回退、复杂文字整形、测量与字形栅格化 | MIT OR Apache-2.0 |
| `image` | 0.25.10 | 有界 PNG/JPEG 解码 | MIT OR Apache-2.0 |
| `arboard` | 3.6.1 | 系统剪贴板 | MIT OR Apache-2.0 |
| `rfd` | 0.17.2 | 原生文件和目录对话框 | MIT |

这些依赖不向言序 API 暴露自身类型或平台句柄。`winit + softbuffer + tiny-skia` 相比
浏览器/WebView 路线保留真正原生桌面窗口；相比首版直接采用 GPU 栈，能在虚拟机、
远程桌面和无硬件加速环境维持同一绘制结果。`cosmic-text` 0.19 使用仍受维护的
HarfRust 整形链，替代旧版已停止维护的 RustyBuzz 链。

## Wayland 安全补丁

crates.io 的 `wayland-scanner 0.31.10` 仍固定 `quick-xml 0.39`，后者受
RUSTSEC-2026-0194 与 RUSTSEC-2026-0195 影响。`vendor/wayland-scanner` 保留其正式
发布源码和 MIT 许可，只把 XML 解析器升级到修复后的 0.41，并采用对应的
`xml10_content` API。补丁不改变生成协议或公开接口；Linux CI 会重新编译并验证
Wayland 与 X11 功能集合。上游发布兼容修复后应移除此临时补丁。

## 有时限的维护例外

`fontdb 0.23.0` 仍传递依赖 `ttf-parser 0.25.1`，RustSec 将整个项目标记为停止维护
（RUSTSEC-2026-0192），但截至 2026-07-20 没有已知漏洞、内存不安全或撤回版本；该
解析器禁止不安全 Rust。言台只在有界字体载入路径和系统字体发现中使用它。因此
`deny.toml` 只对这一项“停止维护”公告给出带理由例外，不忽略漏洞或不健全公告。
每次发布都必须重新审查；`cosmic-text/fontdb` 完成向 Skrifa 的迁移后立即取消例外。

## 发布门禁

```sh
cargo deny --manifest-path yanxu-platform/Cargo.toml check advisories licenses sources
cargo audit --file yanxu-platform/Cargo.lock --deny warnings --ignore RUSTSEC-2026-0192
```

项目许可全文位于 `LICENSE-MIT` 与 `LICENSE-APACHE`。发布包必须同时携带这两份许可、
本说明、`Cargo.lock` 与 `deny.toml`。
