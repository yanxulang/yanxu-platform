# 言台 0.5.0 原生制品来源

版本标签中的六个动态库来自 GitHub Actions `CI` 成功运行
[`29761102761`](https://github.com/yanxulang/yanxu-platform/actions/runs/29761102761)。

| 字段 | 值 |
| --- | --- |
| 工作流 | `CI` |
| 事件 | `pull_request` |
| 源分支 | `feat/platform-0.5-frame-feedback` |
| 源提交 | `a386b9bb9de5a3f3753b0dbf74a511f86c7cda5f` |
| 结论 | `success` |

最终发布提交相对该源提交只固定同次运行生成的原生制品、完整包清单、来源记录和工作流
元数据；Cargo 清单、原生源码和 vendored 构建输入均未改变，示例与测试锁文件只刷新为
固定制品的摘要。六个矩阵项都完成格式、单测、Clippy、Release 构建、ABI 导出、
言序 1.1.7 集成和真实平台检查。

| 目标 | Actions 制品 ID | Actions 制品摘要 | 动态库 SHA-256 | 字节数 |
| --- | ---: | --- | --- | ---: |
| `x86_64-unknown-linux-gnu` | `8468918984` | `8a0e5049e3a436dd5f4f5b64eb887947d82304aaa42ead9b54a9e9f83bb237` | `d627bc0b9290f6023c672bcb52edef2f0ed65cb0dc6f7d14d985193a59a16289` | 10,163,432 |
| `aarch64-unknown-linux-gnu` | `8468912399` | `f9203374000e74d833603a68c17711c65d55ab77723866a420820bc115536ded` | `2a93aabf755666d184630fa4e35b7d15d76ea77407bafc572f656e78978127b9` | 9,839,080 |
| `x86_64-apple-darwin` | `8468946969` | `142719d2cbe0355fa652313ac1b46066973afc727611a613c6723cdce8244ab6` | `898df3f117e9b872b0c4891f973bf0b74902931c7e9623b27ce65119c373357f` | 5,919,112 |
| `aarch64-apple-darwin` | `8468909841` | `719d89f6dc346ae6d531ca4e2fc8dfc7fed7f18db3ee34c777ac185a1da9abe1` | `c0d7858e81c9b59a4e8208be16d63ba06c1ae3caa15ca163d2424c82f85e89fc` | 5,593,840 |
| `x86_64-pc-windows-msvc` | `8468960960` | `0a29b91f005cb274be3f789435aac7795f83d126609b3a6572f56ddd3bb43500` | `f5a0a93afe3c43df2b2699a4165f8dd4c68d0b1dc34c24379266e8bca54ce307` | 4,278,784 |
| `aarch64-pc-windows-msvc` | `8468934921` | `98533858037e39e5c94898dc3568e7f964fcc478143322f52ae7d40d0ae68d7d` | `3cb749e01183f6f884c39562b02dbafeec99d871765e663018fa64a2d6798dff` | 3,680,768 |

CI 每次仍在六个真实目标重新构建和测试。正式 Release 从标签读取上述固定文件，再由
`scripts/verify-release-source.sh`重新计算摘要与大小；这样下游 Git 包和 Release 归档选中
的是完全相同的字节。
