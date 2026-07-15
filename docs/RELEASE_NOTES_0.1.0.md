# 言台 0.1.0

言台是言序 1.1.7 的统一原生桌面平台原语库。它通过 ABI v2 提供真正的原生窗口、事件、
输入法、系统服务、字体、图片和 CPU 二维绘制，不公开系统句柄，也不实现高级控件。

## 新功能

- 应用生命周期、单事件循环、多窗口、定时器、唤醒和单调时间；
- 高 DPI 窗口状态、显示器、主题、光标、拖放和能力查询；
- 键盘、鼠标、触控板、触摸、触控笔、手势、Unicode 文本与中文 IME；
- 剪贴板、打开／保存文件和目录选择；
- 图片解码、自定义字体、字体回退、复杂文字整形、测量和命中；
- 版本化批量事件协议与二进制整帧绘制协议；
- tiny-skia CPU 图形、路径、裁剪、变换、阴影、文字、字形序列与图片；
- 有类型父子资源、回调保留／释放、所有者线程和统一错误模型。

## 修复内容

- 修补 Wayland 构建链的 `quick-xml` 高危公告；
- 升级复杂文字栈并移除停止维护的 RustyBuzz 链；
- 最后一个窗口关闭时补发应用退出请求；
- Linux ARM CI 使用 HTTPS、IPv4 和有界重试，仍在真实网络失败时保持门禁失败。

## 兼容性与平台

最低言序为 1.1.7，言包 0.5.0 无需升级。ABI v2、事件协议 v1.1 和绘制协议 v1.1 在
0.1.x 内按兼容政策演进。正式支持：

- `x86_64-pc-windows-msvc`、`aarch64-pc-windows-msvc`；
- `x86_64-apple-darwin`、`aarch64-apple-darwin`；
- `x86_64-unknown-linux-gnu`、`aarch64-unknown-linux-gnu`。

Linux 同时启用 Wayland 和 X11 回退。六个目标均由对应架构 GitHub 执行器构建和验证。

## 安装示例

```toml
[依赖]
言台 = { 包 = "yanxu-platform", git = "https://github.com/yanxulang/yanxu-platform.git", 修订 = "v0.1.0", 版 = "^0.1" }

[权限]
图形界面 = true
原生扩展 = true
剪贴板 = true
文件对话框 = true
```

随后运行`yanbao 装`和`yanbao 查`。版本标签本身包含完整清单与六目标原生文件，言包按
当前 OS／架构选择并复核 SHA-256 和大小。

## 升级方式

这是 0.1 系列起点。后续更新应修改依赖修订并运行`yanbao 更`；不要手工替换动态库或
编辑锁文件中的原生摘要。既有言窗应用不受影响。

## 制品与校验

Release 附带`yanxu-platform-0.1.0-six-targets.tar.gz`、同名`.sha256`和完整
`yanxu-platform-0.1.0.toml`。
归档包含六个真实动态库、源码、协议/API 文档、测试、许可证和第三方声明。下载后运行
`sha256sum --check yanxu-platform-0.1.0-six-targets.sha256`复核归档；清单还逐项记录每个
动态库的 SHA-256 与字节数。

## 第三方依赖

窗口、表面、二维绘制、文字、字体、图片和系统对话框分别使用固定版本的 winit、
softbuffer、tiny-skia、cosmic-text、fontdb、image 和 rfd。完整版本、许可、维护状态与
唯一公告例外见`docs/THIRD_PARTY.md`和`deny.toml`。

## 已知限制

- 当前统一使用 CPU 绘制，尚无 GPU 后端；
- Linux 文件对话框能力取决于桌面 portal／GTK 环境；
- 系统字体像素和 IME 候选窗外观由各平台决定；
- 重复链接可能含不同的非语义构建元数据，正式字节以标签清单和 Release SHA-256 为准；
- `ttf-parser 0.25.1`仅有停止维护公告、无已知漏洞，已记录并在每次发布复核。
