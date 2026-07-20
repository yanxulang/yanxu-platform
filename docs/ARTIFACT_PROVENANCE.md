# 言台 0.3.0 原生制品来源

版本标签中的六个动态库来自 GitHub Actions `CI` 成功运行
[`29746717854`](https://github.com/yanxulang/yanxu-platform/actions/runs/29746717854)。

| 字段 | 值 |
| --- | --- |
| 工作流 | `CI` |
| 事件 | `pull_request` |
| 源分支 | `feat/platform-0.3-observability` |
| 源提交 | `1e2a15bbda7f038c18b0978915c7872f65b09a63` |
| 结论 | `success` |

最终发布提交相对该源提交只固定同次运行生成的原生制品、完整包清单、来源记录和工作流
元数据；Cargo 清单、原生源码和 vendored 构建输入均未改变，示例与测试锁文件只刷新为
固定制品的摘要。六个矩阵项都完成格式、单测、Clippy、Release 构建、ABI 导出、
言序 1.1.7 集成和真实平台检查。

| 目标 | Actions 制品 ID | Actions 制品摘要 | 动态库 SHA-256 | 字节数 |
| --- | ---: | --- | --- | ---: |
| `x86_64-unknown-linux-gnu` | `8462848607` | `12250b275fba3d4492bf6c96fc1f21a307e83cc2c4dfb47ffdb6a13338623ad1` | `056fab8e91d91e2c126259367263511f211202e3f08d358e3c27d011b292c917` | 9,909,888 |
| `aarch64-unknown-linux-gnu` | `8462839136` | `4f3b75351dbc521625c9687f18e27d9c792ebde63b1fd4eb3750b42f84d6224c` | `aecc6061cdf92234076402967e6443528f5f5ad18c3fa8ecd500198226772949` | 9,578,528 |
| `x86_64-apple-darwin` | `8462945789` | `511358be361a7bff5ab519009be4a8b77b4f89e9593fa31d58384c859a095f7b` | `8984c77de1113a90067d5df69d67c7762efbf6b29d63e8b9052ade3fac67ee5f` | 5,082,068 |
| `aarch64-apple-darwin` | `8462805042` | `00bca3614b075422de331ce9919341466c5a6daed2d08f9b583499ab863067b1` | `4b8d2b464f71dcf11614fe52ece48fe0146fc1f2ca7200188179e0fc749b6121` | 4,795,984 |
| `x86_64-pc-windows-msvc` | `8462896731` | `d15957be2a269946c17afb4e4e2871534d18155136c02029d81ad16f469bf35e` | `8c8ecd8257590d79844f3194da35d6f1460c3a3dd548a32a968136e26c451799` | 4,118,016 |
| `aarch64-pc-windows-msvc` | `8462865168` | `a9221039a33bf1d7a1f7d446b973c22758c3f0b21fc2165eba4c33e590f6d8ab` | `7e955193f91efd2543a736d366b2a2339f9add994e075226ac92b99be7c98f05` | 3,539,968 |

CI 每次仍在六个真实目标重新构建和测试。正式 Release 从标签读取上述固定文件，再由
`scripts/verify-release-source.sh`重新计算摘要与大小；这样下游 Git 包和 Release 归档选中
的是完全相同的字节。
