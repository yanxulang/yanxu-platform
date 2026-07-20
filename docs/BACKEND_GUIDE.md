# 平台后端贡献指南

0.6.0 的 Windows、macOS、Wayland 与 X11 支持共享 `winit`/`softbuffer` 后端。新增系统集成
应先扩展这条路径；只有上游无法表达所需原语时才增加小型 `cfg(target_os)` 适配器。

## 边界

后端可以接触系统窗口、显示器、输入、剪贴板、文件对话框、字体和像素表面，但不得实现
按钮、输入框、布局、主题规则或业务控件，也不得把原生句柄放进 ABI 返回值。

系统事件必须先转换成 [EVENT_PROTOCOL.md](EVENT_PROTOCOL.md) 定义的逻辑像素事件；绘制
必须消费 [DRAW_PROTOCOL.md](DRAW_PROTOCOL.md) 的完整帧。不得为单次指针移动或单条绘制
命令增加 ABI 往返。

## 增加能力的步骤

1. 在无系统句柄的 `model.rs` 中定义必要状态和所有权。
2. 在 `event.rs` 或 `protocol.rs` 中先定义版本与兼容规则。
3. 在 `backend.rs` 添加参数校验、权限检查和稳定错误代码。
4. 若需要言序入口，在 `src/主.yx` 添加最小包装，并重新生成 API 文档。
5. 在 `windowing.rs` 把状态映射到 winit，把系统事件映射回统一事件。
6. 为纯转换、损坏输入、生命周期和错误路径写不需要显示器的单元测试。
7. 为真实窗口路径增加可自动退出的集成示例。
8. 更新能力查询、协议文档、第三方清单和六目标 CI。

ABI 描述符在 `lib.rs` 中集中列出函数和资源类型。新增操作附加到 `Operation` 与函数表，
不要重排已有操作；每次调用先验证 ABI 主机结构、参数数目和资源类型。所有 FFI 入口均由
`catch_unwind` 隔离，新的错误必须映射成静态 `PLATFORM_*` 代码。

## 平台适配原则

- 公开尺寸和位置使用逻辑像素；只有诊断字段明确标注物理像素。
- 平台不能提供的可选信息用空或缺字段，不编造默认设备数据。
- 应先查询 `winit`/现有依赖是否已有跨平台语义，再考虑直接系统 API。
- 必须保持 X11 与 Wayland 同时可编译；Linux 不允许只验证其中一个依赖路径。
- 新依赖固定精确版本，关闭不需要的默认特性，并通过许可与公告检查。
- 系统 API 必需的 `unsafe` 局限在小函数内，写明调用前置条件；工作区拒绝
  `unsafe_op_in_unsafe_fn`。

## 事件

高频事件必须选定合并策略。只有可以安全丢弃中间状态的事件使用`Latest`；滚轮等增量值
需要累积；按键、按钮、IME、拖放和生命周期事件永不合并。任何新事件先加入全量名称测试，
再增加字段与顺序测试。未知事件是上层兼容场景，不能导致整批失败。

## 绘制

新增绘制能力优先增加操作码并提升次版本。负载必须自描述长度、有限、有硬上限且能由旧
后端安全跳过。若改变既有操作码的解释或状态机，则提升主版本。结构化编码器与二进制
渲染器必须有同一测试向量。

CPU 结果测试应断言关键像素/几何关系，不依赖系统字体。如果测试文字像素，加载仓库内
许可明确的固定字体；不能用某台开发机的系统字体制作跨平台金图。

## 本地门禁

从多仓工作区根目录运行：

```sh
cargo fmt --manifest-path yanxu-platform/Cargo.toml --all -- --check
cargo test --manifest-path yanxu-platform/Cargo.toml --workspace --all-targets --locked
cargo clippy --manifest-path yanxu-platform/Cargo.toml --workspace --all-targets --locked -- -D warnings
cargo build --manifest-path yanxu-platform/Cargo.toml --workspace --release --locked
sh yanxu-platform/scripts/prepare-current.sh
yanxu-language-new/target/debug/yanxu 包 更新 yanxu-platform/tests
yanxu-language-new/target/debug/yanxu 字节 yanxu-platform/tests/平台包装.yx
```

还需运行固定版本的 `cargo deny` 与 `cargo audit`。真实窗口示例在当前桌面执行；Linux CI
使用 `xvfb-run`。API 漂移门禁用言序 1.1.7 重新生成 `api/api-v1.json` 和 `docs/API.md`
并逐字节比较。

## 发布后端制品

每个目标只上传一个 Release 动态库和由它生成的目标清单。汇总作业验证六个目标齐全，
重建包含全部原生索引的清单，并以固定时间、所有者和排序创建可复现归档。发布工作流只
接受与成功 CI 运行同一提交的标签，避免把未验证的本地二进制上传为正式制品。
