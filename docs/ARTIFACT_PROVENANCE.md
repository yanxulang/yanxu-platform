# 言台 0.1.0 原生制品来源

版本标签中的六个动态库来自 GitHub Actions `CI` 成功运行
[`29427247716`](https://github.com/yanxulang/yanxu-platform/actions/runs/29427247716)。

| 字段 | 值 |
| --- | --- |
| 工作流 | `CI` |
| 事件 | `pull_request` |
| 源分支 | `fix/linux-arm-package-network` |
| 源提交 | `e4a1b7c7b28bd7a6c229dc19d597e5a2fcf721ca` |
| 结论 | `success` |
| 六目标汇总归档 SHA-256 | `4b27e022a18f5e143ea963c817a2fa33eb8d84e0eea60752c1132aa952f6d8fd` |

最终发布提交相对该源提交只增加发布来源、文档和工作流元数据；Cargo 清单、锁文件、原生
源码和 vendored 构建输入均未改变。六个矩阵项都完成格式、单测、Clippy、Release 构建、
ABI 导出、言序 1.1.7 集成和真实平台检查，随后汇总作业才创建以下文件：

| 目标 | SHA-256 | 字节数 |
| --- | --- | ---: |
| `x86_64-unknown-linux-gnu` | `4eb068b8b779d51940c8d12af26307263de4fb29489d812b7ca6e5b4ac64c068` | 9,358,304 |
| `aarch64-unknown-linux-gnu` | `4b3fe0ff52dd63cd4696f2cde91fdfc47de05878d7f81e480515250803606b79` | 9,584,064 |
| `x86_64-apple-darwin` | `93cba17074d38bd178a99be9e64ddb6b2507bac53827ec801d24ac3997f39253` | 4,939,252 |
| `aarch64-apple-darwin` | `ad91b0a85a9f39926dc2b2127a648b29eaff2e4b3521be893f7c11ec7ffc3c9c` | 4,670,368 |
| `x86_64-pc-windows-msvc` | `6560a8a61a6c9fcc3c94a28e7b668d45944fc611d268c7ef517b79cbec7c17a8` | 4,186,624 |
| `aarch64-pc-windows-msvc` | `be1d8433c0dbcefec6a5c8fe799b35d0490f32f6d169f06ea60206fbc95686bf` | 3,541,504 |

CI 每次仍在六个真实目标重新构建和测试。正式 Release 从标签读取上述固定文件，再由
`scripts/verify-release-source.sh`重新计算摘要与大小；这样下游 Git 包和 Release 归档选中
的是完全相同的字节。
