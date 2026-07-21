# 言台 0.7.0 原生制品来源

版本标签中的六个动态库来自 GitHub Actions `CI` 成功运行
[`29797441928`](https://github.com/yanxulang/yanxu-platform/actions/runs/29797441928)。

| 字段 | 值 |
| --- | --- |
| 工作流 | `CI` |
| 事件 | `pull_request` |
| 源分支 | `feat/platform-0.7-native-accessibility` |
| 源提交 | `43ab3284bb6f9f67f9428c502d01fc9bfaf76490` |
| 结论 | `success` |

最终发布提交相对该源提交只固定同次运行生成的原生制品、完整包清单、来源记录和工作流
元数据；Cargo 清单、原生源码和 vendored 构建输入均未改变，示例与测试锁文件只刷新为
固定制品的摘要。六个矩阵项都完成格式、单测、Clippy、Release 构建、ABI 导出、
言序 1.1.7 集成和真实平台检查。

| 目标 | Actions 制品 ID | Actions 制品摘要 | 动态库 SHA-256 | 字节数 |
| --- | ---: | --- | --- | ---: |
| `x86_64-unknown-linux-gnu` | `8482577162` | `8119e9660520727b27ea5726c3325c5890f409ec474d147c6f9cc0701ba0a041` | `5fe1a22d377d58a12a61baa136b084c4aed725077e210d905fb776440b02afc0` | 15,255,896 |
| `aarch64-unknown-linux-gnu` | `8482577171` | `e2f3b3dd99905ab386bd32ddcac46b7233841d6572c6deaf4152cafe4435f414` | `96760482bb7abb94502cee7f393e4764b5411aef41dac8fc3d51fce07fcdbdaa` | 14,477,344 |
| `x86_64-apple-darwin` | `8482642340` | `03a975bbd5c1353a2ae704a1f630058510b529655e40516479c0c536ede5afd3` | `9fa8ebd27329ab086bce794c4a96ff5264a6f09f0b7e42754bd51e4dd3840237` | 6,462,032 |
| `aarch64-apple-darwin` | `8482564915` | `ece699072375578b02561287231dacbfab85802200b7c402394b5f56dced1ac5` | `6a6b8a51058d1eb3a309058deb37ea623b3ac2be83137955719b64fef4ebb51d` | 6,079,872 |
| `x86_64-pc-windows-msvc` | `8482595655` | `02f9f35a919680ec7f70eb8b5d4f8fe3a51d50786c07eba087aecf15283e9a81` | `0b9c4b5e527ab43029e32940554837b082604a4722c28e1c606d6a259ffdd2ea` | 4,910,592 |
| `aarch64-pc-windows-msvc` | `8482586260` | `e1ed3bb6160d7e30e9657d27fd124c1deb62432e52e33458a9cd9d5bf4dcdd86` | `42e59420c80b6f7ff5e1d4fa8e75fc020eafe22e9965df5123b1c79bc0d54848` | 4,214,272 |

CI 每次仍在六个真实目标重新构建和测试。正式 Release 从标签读取上述固定文件，再由
`scripts/verify-release-source.sh`重新计算摘要与大小；这样下游 Git 包和 Release 归档选中
的是完全相同的字节。
