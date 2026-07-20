# 言台 0.4.0 原生制品来源

版本标签中的六个动态库来自 GitHub Actions `CI` 成功运行
[`29754666670`](https://github.com/yanxulang/yanxu-platform/actions/runs/29754666670)。

| 字段 | 值 |
| --- | --- |
| 工作流 | `CI` |
| 事件 | `pull_request` |
| 源分支 | `feat/platform-0.4-clipboard-data` |
| 源提交 | `1f704acd4c13f521f7fe36428b46fc8d5708eeac` |
| 结论 | `success` |

最终发布提交相对该源提交只固定同次运行生成的原生制品、完整包清单、来源记录和工作流
元数据；Cargo 清单、原生源码和 vendored 构建输入均未改变，示例与测试锁文件只刷新为
固定制品的摘要。六个矩阵项都完成格式、单测、Clippy、Release 构建、ABI 导出、
言序 1.1.7 集成和真实平台检查。

| 目标 | Actions 制品 ID | Actions 制品摘要 | 动态库 SHA-256 | 字节数 |
| --- | ---: | --- | --- | ---: |
| `x86_64-unknown-linux-gnu` | `8466274736` | `365bd397a2f099cd8e3d7f848c733c9ad88dad9a60f60dcf61779df7bca123fd` | `129a40e3718eccce0013716503b81b88088efc2ac71f652f0cada63717ae214c` | 10,151,928 |
| `aarch64-unknown-linux-gnu` | `8466246105` | `8ee690245a5dbcd70898b35720459e7f4c9a5ddd3a89c09aaa38c41c6ddc7122` | `438987837d2f71cb2a3564a92aba4060e3a4c4467f9227e9a0477ee51e7c55cf` | 9,771,848 |
| `x86_64-apple-darwin` | `8466366269` | `41f06223d289e8894e00b9eaa483c5d6e9dcba47d8869b37b1c5da16aa0d6d19` | `753f8c97b859f661aeef4623bc03ec152b070220cc294c24e82d1470608f2841` | 5,914,200 |
| `aarch64-apple-darwin` | `8466254670` | `4fe63a325ab4b04e6f963490fc28810cf483d43776fd3581947b72a4804df3d3` | `bc43efcd9626289e330e2c533a9c86af3bd15ef0980b76aea32fb338fb7472ec` | 5,576,368 |
| `x86_64-pc-windows-msvc` | `8466223467` | `ab9115dd9b573359799dd590f91da449deab4ee8677aafbfad67f0975bc0138c` | `0606d427c4bd67d7abe826e56e9892dcc10f456a71ae9efb8dce0af404d236e1` | 4,272,640 |
| `aarch64-pc-windows-msvc` | `8466265216` | `7e651e1a5c4b14e50874c34cba64a6d0f6a9d417fee2cb25c6af2d378dea77d4` | `cad2b8627e9b5588a2c250660154c2dafa3272d316f75a913978c6339392acbe` | 3,680,256 |

CI 每次仍在六个真实目标重新构建和测试。正式 Release 从标签读取上述固定文件，再由
`scripts/verify-release-source.sh`重新计算摘要与大小；这样下游 Git 包和 Release 归档选中
的是完全相同的字节。
