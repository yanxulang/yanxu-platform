# 第三方依赖、许可与维护状态

言台原生后端只固定使用下列直接依赖。正式构建必须使用 `Cargo.lock` 和 `--locked`；
六个目标的完整传递图由 `deny.toml` 约束为兼容许可和 crates.io 来源。

| 依赖 | 固定版本 | 用途 | 许可 |
| --- | --- | --- | --- |
| `accesskit` | 0.24.1 | 跨 UIA、NSAccessibility 与 AT-SPI 的语义节点和动作模型 | MIT OR Apache-2.0 |
| `accesskit_winit` | 0.33.2 | 原生窗口适配器生命周期、激活、树同步和动作代理 | Apache-2.0 |
| `winit` | 0.30.13 | Windows、macOS、Wayland、X11 窗口与输入事件 | Apache-2.0 |
| `softbuffer` | 0.4.8 | 无 GPU 要求的原生窗口 CPU 表面 | MIT OR Apache-2.0 |
| `tiny-skia` | 0.11.4 | 跨平台一致的二维栅格化 | BSD-3-Clause |
| `cosmic-text` | 0.19.0 | 字体回退、复杂文字整形、测量与字形栅格化 | MIT OR Apache-2.0 |
| `image` | 0.25.10 | 有界 PNG/JPEG 解码 | MIT OR Apache-2.0 |
| `arboard` | 3.6.1 | 有界 UTF-8 与 RGBA8 系统剪贴板 | MIT OR Apache-2.0 |
| `rfd` | 0.17.2 | 原生文件和目录对话框 | MIT |
| `unicode-segmentation` | 1.13.3 | Unicode 字素到 UTF-8 无障碍选区边界 | MIT OR Apache-2.0 |

这些依赖不向言序 API 暴露自身类型或平台句柄。`winit + softbuffer + tiny-skia` 相比
浏览器/WebView 路线保留真正原生桌面窗口；相比首版直接采用 GPU 栈，能在虚拟机、
远程桌面和无硬件加速环境维持同一绘制结果。`cosmic-text` 0.19 使用仍受维护的
HarfRust 整形链，替代旧版已停止维护的 RustyBuzz 链。

1.0.0 为 `accesskit_winit` 精确启用 `rwh_06`、Unix 适配器与 `async-io` 执行器；Windows
和 macOS 由目标依赖分别解析到 UIA 与 NSAccessibility，Linux 解析到 AT-SPI。它与现有
`winit 0.30.13` 合并为同一窗口依赖，不引入第二套事件循环。`unicode-segmentation`只在
已通过 65536 字节单字段上限的输入值上运行，异常长字素会有界回退为 Unicode 标量。

1.0.0 继续为 `arboard` 启用 `image-data` 功能，以便在六个目标读取和写入 RGBA8 图片。
该功能增加 `tiff` 及其 `fax`、`half`、`weezl` 等传递构建依赖；言台不向公开 API 暴露
这些编解码器，也不接受 TIFF 字节输入。所有版本由 `Cargo.lock` 固定并进入同一许可、
来源与公告门禁。

## Wayland 安全补丁

crates.io 的 `wayland-scanner 0.31.10` 仍固定 `quick-xml 0.39`，后者受
RUSTSEC-2026-0194 与 RUSTSEC-2026-0195 影响。`vendor/wayland-scanner` 保留其正式
发布源码和 MIT 许可，只把 XML 解析器升级到修复后的 0.41，并采用对应的
`xml10_content` API。补丁不改变生成协议或公开接口；Linux CI 会重新编译并验证
Wayland 与 X11 功能集合。上游发布兼容修复后应移除此临时补丁。

## fontdb 上游修复固定

`fontdb 0.23.0` 的 crates.io 包仍传递依赖已经停止维护的 `ttf-parser 0.25.1`
（RUSTSEC-2026-0192）。言台不再忽略该公告，而是通过 `[patch.crates-io]` 固定到
`fontdb` 上游已合并并获批准的 PR #92 合并提交
`aaf5220300454ce30d07fb88d9722927521b7799`。该提交把字体发现所需的名称、语言和 OS/2
解析最小子集纳入仍在维护的 `fontdb`，并从依赖图移除 `ttf-parser`。

`deny.toml` 只允许 `https://github.com/RazrFalcon/fontdb` 这一 Git 来源，并强制所有 Git
依赖使用完整 `rev` 规格；锁文件再固定实际提交。上游已说明将发布包含该修复的新版本，
正式包可用后必须切回 crates.io 并删除 Git 来源许可。

2026-07-21 为 1.0 再次复核：crates.io 的`newest_version`与`max_version`仍均为 0.23.0，
尚无包含 PR #92 的正式包；带
`--manifest-path yanxu-platform/Cargo.toml --locked -p fontdb`的`cargo tree`确认当前解析
到上述完整提交，反向查询`ttf-parser`则确认依赖图中不存在该包。因此 1.0 继续采用 Git
固定，不以回退到已知停止维护依赖来换取纯 registry 来源。

## 发布门禁

```sh
cargo deny --manifest-path yanxu-platform/Cargo.toml check advisories licenses sources
cargo audit --file yanxu-platform/Cargo.lock --deny warnings
```

项目许可全文位于 `LICENSE-MIT` 与 `LICENSE-APACHE`。发布包必须同时携带这两份许可、
本说明、`Cargo.lock` 与 `deny.toml`。
