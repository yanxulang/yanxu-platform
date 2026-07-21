# 言台 0.8.0 原生制品来源

版本标签中的六个动态库来自 GitHub Actions `CI` 成功运行
[`29803668169`](https://github.com/yanxulang/yanxu-platform/actions/runs/29803668169)。

| 字段 | 值 |
| --- | --- |
| 工作流 | `CI` |
| 事件 | `pull_request` |
| 源分支 | `feat/platform-0.8-resource-quotas-shutdown` |
| 源提交 | `d68bb7297d357b967188362afd6b60f13b6e843f` |
| 结论 | `success` |

最终发布提交相对该源提交只固定同次运行生成的原生制品、完整包清单、来源记录和工作流
元数据；Cargo 清单、原生源码和 vendored 构建输入均未改变，示例与测试锁文件只刷新为
固定制品的摘要。六个矩阵项都完成格式、单测、Clippy、Release 构建、ABI 导出、
言序 1.1.7 集成和真实平台检查。

| 目标 | Actions 制品 ID | Actions 制品摘要 | 动态库 SHA-256 | 字节数 |
| --- | ---: | --- | --- | ---: |
| `x86_64-unknown-linux-gnu` | `8484759724` | `6905cb412a89771bc17d36fd227ca0f4f7a90038b9b9967ee3e8a5909c20dac9` | `9708f26e4e0b5e1547ef739e591ceed15bca7efa6713263ecaf0b2cb9a6cc23d` | 15,306,520 |
| `aarch64-unknown-linux-gnu` | `8484751949` | `206077647968034038fd858dfd5959282f869b42de59ba27c62e6baba7c4ba25` | `8204b4ce4bd0ac52259ec3b0a4d45153bd78c6bc9ae6fca411dd8aaa04780b9e` | 14,556,288 |
| `x86_64-apple-darwin` | `8484775731` | `ffbe764eac308b3083d2ff320388427899ab960bd498ca2c4e837ba456f231b3` | `468c699b530c18ace779d442150d4c4f90018b71cbf8fedaa76510cb301fec69` | 6,520,368 |
| `aarch64-apple-darwin` | `8484752935` | `5bf1a3e93d58f3c5047416288b07d404486602f1d34344fcd93bed17b7a4754f` | `245b31e3d7fd855716c0e8e888c23d4d4094c8ac10d7129c012b91f1d57086fc` | 6,129,840 |
| `x86_64-pc-windows-msvc` | `8484778764` | `e1880e19df5b6e903c4bfeeef0ca55a1d7bdc83a5566fdc6b1c844898cd8f139` | `70342522203e511ef366e81bd70673a9982c9eea6f7952c54dda318f012ad8f3` | 4,951,552 |
| `aarch64-pc-windows-msvc` | `8484758349` | `7cee357f98073dd5617802bd9368ed356cf763326eec6fa2ceba5ba604d426cc` | `8d3af02c5f20b69439ca02d43db7113831c7d03d8cb440e7dcfe4e9cf6157ecb` | 4,254,208 |

CI 每次仍在六个真实目标重新构建和测试。正式 Release 从标签读取上述固定文件，再由
`scripts/verify-release-source.sh`重新计算摘要与大小；这样下游 Git 包和 Release 归档选中
的是完全相同的字节。
