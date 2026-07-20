# 言台（yanxu-platform）

言台是言序的新原生跨平台平台接口库。它向上层提供应用、窗口、事件、输入、文本、
图片、系统服务和二维绘制等平台原语，不公开 `HWND`、`NSWindow`、Wayland/X11 对象或
任何后端指针，也不实现按钮、输入框、列表等高级控件。

言台与现有 [`yanxu-gui`](https://github.com/yanxulang/yanxu-gui) 并行存在。言窗继续保持
原有 API；新路线为：

```text
言序应用 → yanxu-ui（言界）→ yanxu-platform（言台）→ 原生平台后端 → 操作系统
```

当前版本为 `0.4.0`，最低要求言序 `1.1.7`。原生后端采用 ABI v2，窗口层使用
`winit`，CPU 表面使用 `softbuffer`，二维栅格化使用 `tiny-skia`，复杂文字使用
`cosmic-text`。依赖版本、许可证与选型理由会固定在 `docs/` 中。

0.4.0 把剪贴板扩展为可协商的有界数据接口：文字限制为 16 MiB UTF-8，图片使用严格
验证的 RGBA8、单边不超过 16384 且总内容不超过 256 MiB。应用可在写入前从能力查询读取
支持格式和精确上限；系统返回的数据也会经过同一套边界检查。

## 安装依赖

正式发布后，在言序项目目录使用言包添加 GitHub 包：

```sh
yanbao 加 yanxulang/yanxu-platform
yanbao 装
```

项目清单必须授予原生窗口权限；使用相应系统服务时再加入细分权限：

```toml
[权限]
图形界面 = true
原生扩展 = true
剪贴板 = true
文件对话框 = true
```

Release 包同时包含 Windows、macOS 与 Linux 的 x86-64/ARM64 原生制品。言序按当前
OS/架构选择文件，并验证清单中的 SHA-256 与大小。

## 从源码验证

所有命令从包含本仓库的多仓工作区根目录执行：

```sh
cargo fmt --manifest-path yanxu-platform/Cargo.toml --all -- --check
cargo clippy --manifest-path yanxu-platform/Cargo.toml --workspace --all-targets --locked -- -D warnings
cargo test --manifest-path yanxu-platform/Cargo.toml --workspace --all-targets --locked
```

本仓库使用 MIT 或 Apache-2.0 双许可证。

## 最小用法

应用必须显式授予`图形界面`和`原生扩展`权限。事件回调一次接收一个批次；窗口只接收
完整帧，不暴露任何平台句柄：

```yanxu
引「包:言台」为 平台；

法 处理事件（批次：典）则
    # 在“需要重绘”事件中构造完整命令列并调用窗口.提交帧（…）。
终

定 应用 为 平台.应用（「示例」，处理事件）；
定 窗口 为 应用.窗口（{「标题」：「你好」，「宽」：640，「高」：420}）；
应用.运行（）；
```

可运行的完整代码见 [`examples/最小窗口.yx`](examples/最小窗口.yx)。主要文档：

- [实际架构](docs/ARCHITECTURE.md)与[平台 API](docs/PLATFORM_API.md)
- [事件协议](docs/EVENT_PROTOCOL.md)、[绘制协议](docs/DRAW_PROTOCOL.md)和[文字/IME](docs/TEXT_AND_IME.md)
- [资源生命周期](docs/RESOURCE_LIFETIME.md)、[线程模型](docs/THREADING_MODEL.md)与[兼容政策](docs/COMPATIBILITY.md)
- [后端贡献](docs/BACKEND_GUIDE.md)、[打包发布](docs/PACKAGING.md)和[生成的 API 参考](docs/API.md)
- [第三方许可与安全审计](docs/THIRD_PARTY.md)、[原生制品来源](docs/ARTIFACT_PROVENANCE.md)

架构决策记录位于 `docs/ADR-001` 至 `ADR-004`。
