# 言台 0.2.0 原生制品来源

版本标签中的六个动态库来自 GitHub Actions `CI` 成功运行
[`29740577228`](https://github.com/yanxulang/yanxu-platform/actions/runs/29740577228)。

| 字段 | 值 |
| --- | --- |
| 工作流 | `CI` |
| 事件 | `pull_request` |
| 源分支 | `feat/platform-0.2-resilience` |
| 源提交 | `0ac6bec20d22e55831452b86a69a6729660f4bd3` |
| 结论 | `success` |

最终发布提交相对该源提交只固定同次运行生成的原生制品、完整包清单、来源记录和工作流
元数据；Cargo 清单、原生源码和 vendored 构建输入均未改变，示例与测试锁文件只刷新为
固定制品的摘要。六个矩阵项都完成格式、单测、Clippy、Release 构建、ABI 导出、
言序 1.1.7 集成和真实平台检查。

| 目标 | Actions 制品 ID | Actions 制品摘要 | 动态库 SHA-256 | 字节数 |
| --- | ---: | --- | --- | ---: |
| `x86_64-unknown-linux-gnu` | `8460284360` | `55b2ad3372fc9ddb642675313c2c1abae6ef9255915fba9f3358b073175fde54` | `bfb591e42594af0a080a3689c02a14ae87ce839e852623d5fed722b2a389b442` | 9,912,272 |
| `aarch64-unknown-linux-gnu` | `8460289671` | `b3f1341f6527b34803e0a981b073e934b86b7e5143a8c2555e77c8bbc21e9350` | `125fb0088060aec29ca690111fc2c4439c54846ea97ac2761b99f0136fc40026` | 9,580,616 |
| `x86_64-apple-darwin` | `8460456670` | `5e7ef90b07997271ccc65780cedabd469401904831fbdbaf62a19d4ece675cbd` | `66f8f24e44e6ccf4fcf088352875663c92de61949e3a27ff8ec2ceb057e88246` | 5,075,380 |
| `aarch64-apple-darwin` | `8460258892` | `255e3ee87bf903446e2d60ce028b1d1c0f15f1c1a6fd032430dc91243b29782d` | `8c13d866c5ba3ee67ac4a2b0e14a01c4c46da3218a3a3300a0dd750ed8e13a82` | 4,794,784 |
| `x86_64-pc-windows-msvc` | `8460332318` | `21ac7a9e9ac04019569ec2b618974a7f937679d555ce772d5303acab1147786f` | `81f030e4a436ee8c8d5f227ce1aa3b3ad86ae0bf1e2f4bd66ebb2b121c255b2a` | 4,112,384 |
| `aarch64-pc-windows-msvc` | `8460310361` | `c1255f53d00d93867021d7af606ecb2fdd382810f7529212e04580752516dd65` | `98181f7bddfd9eb8d43d7c287512cffb91535ec183179d524e23cdc4960ac3b0` | 3,535,360 |

CI 每次仍在六个真实目标重新构建和测试。正式 Release 从标签读取上述固定文件，再由
`scripts/verify-release-source.sh`重新计算摘要与大小；这样下游 Git 包和 Release 归档选中
的是完全相同的字节。
