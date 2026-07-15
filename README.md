# 言台（yanxu-platform）

言台是言序的新原生跨平台平台接口库。它向上层提供应用、窗口、事件、输入、文本、
图片、系统服务和二维绘制等平台原语，不公开 `HWND`、`NSWindow`、Wayland/X11 对象或
任何后端指针，也不实现按钮、输入框、列表等高级控件。

言台与现有 [`yanxu-gui`](https://github.com/yanxulang/yanxu-gui) 并行存在。言窗继续保持
原有 API；新路线为：

```text
言序应用 → yanxu-ui（言界）→ yanxu-platform（言台）→ 原生平台后端 → 操作系统
```

当前开发版本为 `0.1.0`，最低要求言序 `1.1.7`。原生后端采用 ABI v2，窗口层使用
`winit`，CPU 表面使用 `softbuffer`，二维栅格化使用 `tiny-skia`，复杂文字使用
`cosmic-text`。依赖版本、许可证与选型理由会固定在 `docs/` 中。

## 从源码验证

所有命令从包含本仓库的多仓工作区根目录执行：

```sh
cargo fmt --manifest-path yanxu-platform/Cargo.toml --all -- --check
cargo clippy --manifest-path yanxu-platform/Cargo.toml --workspace --all-targets --locked -- -D warnings
cargo test --manifest-path yanxu-platform/Cargo.toml --workspace --all-targets --locked
```

本仓库使用 MIT 或 Apache-2.0 双许可证。
