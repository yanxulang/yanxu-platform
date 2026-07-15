//! 言台原生后端。
//!
//! 公开边界是言序 ABI v2；Rust 模块同时作为 `rlib` 构建，以便协议、资源与无显示
//! 模型能够由普通单元测试覆盖。

pub mod data;
pub mod event;
pub mod model;
pub mod protocol;
