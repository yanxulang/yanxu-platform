# 言台 1.0.0 原生制品来源

版本标签中的六个动态库来自 GitHub Actions `CI` 成功运行
[`29816339755`](https://github.com/yanxulang/yanxu-platform/actions/runs/29816339755)。

| 字段 | 值 |
| --- | --- |
| 工作流 | `CI` |
| 事件 | `pull_request` |
| 源分支 | `feat/platform-1.0-api-freeze` |
| 源提交 | `36e70610536791d12afe70abdfb0316d4b002c4a` |
| 结论 | `success` |

最终发布提交相对该源提交只固定同次运行生成的原生制品、完整包清单、来源记录、工作流
来源钉住值和刷新后的示例/测试锁文件；Cargo 清单、依赖锁、原生源码、测试和 vendored
构建输入均未改变。六个矩阵项都完成格式、109 项 Rust 门禁、Clippy、Release 构建、
ABI 导出、言序 1.1.7 集成和真实平台窗口/原生无障碍桥检查。

| 目标 | Actions 制品 ID | Actions 制品摘要 | 动态库 SHA-256 | 字节数 |
| --- | ---: | --- | --- | ---: |
| `x86_64-unknown-linux-gnu` | `8489620360` | `73b923ff0787563c27085522e5751a6695165c1d76df99192048c44321dccdcb` | `5a6486c035b79b5140cbea8e528bdaa7439f8155072b01780e67c82b4971c751` | 15,304,688 |
| `aarch64-unknown-linux-gnu` | `8489647335` | `30765cac6834f0a5e684df9e8f7c1a55b2b41bf6c60abac962581f96c61d339e` | `e642b292d25e0498a60841a95256ad37c2cbff976d5d919f2406b8c0800f6caa` | 14,551,928 |
| `x86_64-apple-darwin` | `8489636273` | `512daca11a40d36ae7deb04918146338e9fd6c68b8a2f3a8117182a60e7456a0` | `390679cbb529595409dc60c20203cf051d67b5e33ce29b9154226212ea588a13` | 6,524,304 |
| `aarch64-apple-darwin` | `8489595345` | `0f07ba363e86a9a6d23e4d51875e997fecaa0ebf25681e16f0b20a5fd7559734` | `4ba2a55db0828178d67c555500844bc2e03856bef79aeddae96bf0d3bfec6fdb` | 6,129,760 |
| `x86_64-pc-windows-msvc` | `8489689480` | `d8e8e88f9373d9979956227d4b8fefa267ee25ba799c2bb6cc16a18c3df86389` | `be26a1098285e12ae2da7b42d877c468dad543c7ffda826e4cb4495029cfffe6` | 4,951,552 |
| `aarch64-pc-windows-msvc` | `8489649535` | `171958c9a8d0a43368dadefbc67fb2b13c2994b9ef28a83aba74dc2dbfd2c398` | `f4e35ee24d024924198b74c723fa43d78b58abdc8a70b13948f9579ddc55ff0a` | 4,254,720 |

同次运行的固定 Linux x86-64 性能报告来自 Actions 制品 `8489573456`，制品摘要为
`6546ace1e4e923aa2ce0f3f2818a3110ff7ee8b7316e7c0216c7655643df3064`。四项中位数分别为
44.081 ns/绘制命令、186.535 ns/事件、209.272 ns/资源和 48,884.665 ns/CPU 帧；MAD
为 0.018%–0.057%，全部低于预算和 20% 稳定性上限。

CI 每次仍在六个真实目标重新构建和测试。正式 Release 从标签读取上述固定文件，再由
`scripts/verify-release-source.sh`重新计算摘要与大小；这样下游 Git 包和 Release 归档
选中的是完全相同的字节。标签 CI 还会重新执行性能与零例外依赖策略门禁，Release 工作流
只接受标签同一提交的成功 CI 候选。
