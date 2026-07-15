# 构建、打包与发布

## 开发构建

从包含各独立仓库的多仓工作区根目录运行：

```sh
cargo build --manifest-path yanxu-platform/Cargo.toml --workspace --release --locked
sh yanxu-platform/scripts/prepare-current.sh
```

脚本读取 `rustc -vV` 的真实宿主目标，只复制本次实际构建的一个动态库到
`dist/<target>/`，计算 SHA-256 与大小，并从`言序.toml.in`生成当前机器可执行的
`言序.toml`。脚本不会声称生成其他架构制品。

随后用言序 1.1.7 更新测试锁并执行真实 ABI：

```sh
yanxu-language-new/target/debug/yanxu 包 更新 yanxu-platform/tests
yanxu-language-new/target/debug/yanxu 字节 yanxu-platform/tests/平台包装.yx
yanxu-language-new/target/debug/yanxu 包 更新 yanxu-platform/examples
yanxu-language-new/target/debug/yanxu 字节 yanxu-platform/examples/最小窗口.yx
```

## 六目标包

CI 的六个原生矩阵项各自构建、测试并上传一个目标目录。`assemble` 作业只有在全部目标和
依赖审计成功后才：

1. 验证每个目标目录恰有一个动态库；
2. 重建含六项`[原生.<系统>.<架构>]`的完整清单；
3. 为每个文件写入真实 SHA-256 与字节数；
4. 以固定路径排序、Unix epoch 时间、uid/gid 0 创建归档；
5. 上传归档、独立摘要和完整清单为同一 CI 候选制品。

归档名为`yanxu-platform-0.1.0-six-targets.tar.gz`，支持：

```text
windows/x64, windows/arm64
macos/x64, macos/arm64
linux/x64, linux/arm64
```

正式版本还把同一批已验证制品和完整`言序.toml`纳入版本标签，使言包的 Git 依赖可以在
六个目标上直接解析，而不依赖未定义的外部下载步骤。`dist/`中的文件不得由单机本地构建
替换；它们来自六目标成功 CI 候选，清单逐项固定 SHA-256 和字节数。开发脚本生成的单目标
清单仅用于本地测试，发布提交必须恢复完整六目标索引。

## 应用 Bundle

使用言台的应用在清单声明依赖和权限后，由言包完成目标平台 Bundle：

```sh
yanbao 加 yanxulang/yanxu-platform
yanbao 装
yanbao 查
yanbao 构 --release --bundle
```

言包根据锁定依赖图把当前目标的动态库纳入 macOS `.app`、Windows GUI 应用目录或 Linux
AppDir。应用不应手工写死 `.dll`、`.dylib` 或 `.so` 路径。跨架构发布必须在对应真实
执行器构建或使用言台 Release 中已经验证的精确目标制品。

## 正式发布门禁

Release 工作流不重新编译。维护者先把版本提交合并到主分支，等待同一提交的`CI`成功，
再创建签名策略允许的不可移动标签，并手动触发`Release`，输入标签和成功 CI run ID。

工作流验证：

- 标签提交等于该 CI 的 `headSha`；
- 工作流名确为`CI`且结论为 success；
- 下载的是该 run 的`yanxu-platform-six-targets`；
- 上传前重新校验归档 SHA-256；
- `gh release create --verify-tag`成功。

因此本地临时构建不能直接成为正式 Release。发布说明必须同步 `CHANGELOG.md`、兼容政策、
安装方式、API/协议文档和示例。
