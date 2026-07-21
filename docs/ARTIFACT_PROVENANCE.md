# 言台 0.9.0 原生制品来源

版本标签中的六个动态库来自 GitHub Actions `CI` 成功运行
[`29808592375`](https://github.com/yanxulang/yanxu-platform/actions/runs/29808592375)。

| 字段 | 值 |
| --- | --- |
| 工作流 | `CI` |
| 事件 | `pull_request` |
| 源分支 | `feat/platform-0.9-hardening` |
| 源提交 | `3d1dae4b4546762902e99d37b517f383c8a7a521` |
| 结论 | `success` |

最终发布提交相对该源提交只固定同次运行生成的原生制品、完整包清单、来源记录、工作流
来源钉住值和刷新后的示例/测试锁文件；Cargo 清单、依赖锁、原生源码、测试和 vendored
构建输入均未改变。六个矩阵项都完成格式、106 项 Rust 门禁、Clippy、Release 构建、
ABI 导出、言序 1.1.7 集成和真实平台窗口/原生无障碍桥检查。

| 目标 | Actions 制品 ID | Actions 制品摘要 | 动态库 SHA-256 | 字节数 |
| --- | ---: | --- | --- | ---: |
| `x86_64-unknown-linux-gnu` | `8486604968` | `caee9bd463c0f79d8d298a3978d35ea51bdc2b65e5aa041d535ba655d0622df5` | `4d321de2e9176c0db73f0f1f7f5e44b6da1e648c379cb1d7bf9e81bc3bebcf4a` | 15,304,480 |
| `aarch64-unknown-linux-gnu` | `8486601032` | `3326cc84eeaf2dac72123b016461d2dd585ea513369d22f42fc11da7c873dd88` | `75f14e39003d06fd1e8c25244ebdb1ce28801c761d3aaf9c7a9ce7a166600a1d` | 14,553,968 |
| `x86_64-apple-darwin` | `8486606130` | `93e1b157416a8001587ffb25af05d8fcd980b6222b0d0019108b268a6d69003e` | `96f604c01981e7cdbec5e79e7bfa6b4f806b87b6d79e391d29d6a2f0c03014bd` | 6,524,288 |
| `aarch64-apple-darwin` | `8486584283` | `d286c82b6aaaf42eb9a49aa0f5168a0cd21113f6822c763eb724a93efb8d03da` | `12859a10f5e24b402b481d890f5dafaa8ba1be71b8f99a3366037042f8ea3bf4` | 6,129,760 |
| `x86_64-pc-windows-msvc` | `8486644071` | `bdbb305f707bb1cc034496cb3855143b5883f0cd7e6944b30be7b960f42846ac` | `8ce2ea3f6f09f7c45129207c1fbb6b3cc272b2dd7e1d5e8686b805d12fe18859` | 4,951,552 |
| `aarch64-pc-windows-msvc` | `8486609782` | `9266ac9ddb1eb1cea835b9c1d58143c8744f193ce08f73b73fcae136347d65f7` | `d624758e1ca47f589168ca8fdbe2f14bb4f0b9d2d98bb29e06a13de168db5973` | 4,255,232 |

同次运行的固定 Linux x86-64 性能报告来自 Actions 制品 `8486541713`，制品摘要为
`6609b0da17cb0e803029fd46f91333e7318492794bdf3cac7ec056d23d148239`。四项中位数分别为
37.444 ns/绘制命令、154.308 ns/事件、173.562 ns/资源和 45,478.173 ns/CPU 帧；MAD
为 0.228%–1.014%，全部低于预算和 20% 稳定性上限。

CI 每次仍在六个真实目标重新构建和测试。正式 Release 从标签读取上述固定文件，再由
`scripts/verify-release-source.sh`重新计算摘要与大小；这样下游 Git 包和 Release 归档
选中的是完全相同的字节。标签 CI 还会重新执行性能与零例外依赖策略门禁，Release 工作流
只接受标签同一提交的成功 CI 候选。
