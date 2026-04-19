//! MCP (Model Context Protocol) 服务器管理模块
//!
//! 本模块负责 MCP 服务器配置的验证、同步和导入导出。
//!
//! ## 模块结构
//!
//! - `validation` - 服务器配置验证
//! - `claude` - Claude MCP 同步和导入

mod claude;
mod validation;

// 重新导出公共 API
pub use claude::{
    import_from_claude, remove_server_from_app, remove_server_from_claude, sync_enabled_to_claude,
    sync_single_server_to_app, sync_single_server_to_claude,
};
