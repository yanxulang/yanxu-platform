# 言台 0.6.0 原生制品来源

版本标签中的六个动态库来自 GitHub Actions `CI` 成功运行
[`29768490886`](https://github.com/yanxulang/yanxu-platform/actions/runs/29768490886)。

| 字段 | 值 |
| --- | --- |
| 工作流 | `CI` |
| 事件 | `pull_request` |
| 源分支 | `feat/platform-0.6-accessibility-protocol` |
| 源提交 | `66bbe967eb10d10276d9d4a8ba5aa34561e632cb` |
| 结论 | `success` |

最终发布提交相对该源提交只固定同次运行生成的原生制品、完整包清单、来源记录和工作流
元数据；Cargo 清单、原生源码和 vendored 构建输入均未改变，示例与测试锁文件只刷新为
固定制品的摘要。六个矩阵项都完成格式、单测、Clippy、Release 构建、ABI 导出、
言序 1.1.7 集成和真实平台检查。

| 目标 | Actions 制品 ID | Actions 制品摘要 | 动态库 SHA-256 | 字节数 |
| --- | ---: | --- | --- | ---: |
| `x86_64-unknown-linux-gnu` | `8471860229` | `faf21b054c9a2556d289fa2e77bad3dddfb6ca5a27d2c8601ef31f2fe1702af5` | `f49c6ba79657f8369c614dad921a806187b80f41948aae6b52aca48400931279` | 10,263,464 |
| `aarch64-unknown-linux-gnu` | `8471877763` | `77f1123d629a250456d10159f292f58ab82d6ec60174ef66232eefee9bafe568` | `5643d802a1215e398e00f6c78db6641d0f7fea023b6955585018c805d8bd0835` | 9,862,864 |
| `x86_64-apple-darwin` | `8471880340` | `31216b2c18f3eeead3d2f7982b02f012e85a6a1734b31f418442b8eca4779aec` | `0a419dc2ac43c12ee1b81bc47f4a0e520a2845c1a1a72db675b3fd650580815c` | 6,002,464 |
| `aarch64-apple-darwin` | `8471886622` | `6042ba41ad3f7f903dbe1568408ffd9d261dc09b312bd6b867f88f87befa96c6` | `85b75a3b1a07c34ad1a08c4867ca4c5e35f309e85b8054a10f0c5a238bf2385a` | 5,654,224 |
| `x86_64-pc-windows-msvc` | `8471933362` | `d9499871492a0002815aba513e81355e3c6598e8a7f03cc2c9b599113f32a7c4` | `7c88764a3152390a91e45a50c666835889114ae0a19dd79e3f1c8451512c1a88` | 4,365,824 |
| `aarch64-pc-windows-msvc` | `8471885187` | `b756c983521a24eae738e997de029a5d1d12f9aeebcb88b828667bbee1b1bffc` | `8dea75404106d7058315500fd1234695a67776d8176748e3e92edaf8bb02ae1a` | 3,756,544 |

CI 每次仍在六个真实目标重新构建和测试。正式 Release 从标签读取上述固定文件，再由
`scripts/verify-release-source.sh`重新计算摘要与大小；这样下游 Git 包和 Release 归档选中
的是完全相同的字节。
