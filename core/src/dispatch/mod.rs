//! 服务端方法路由模块。
//!
//! 负责按 service 注册 handler，并把未知方法映射为 UNIMPLEMENTED。

mod dispatcher;

pub use dispatcher::{DispatchStatus, Dispatcher, ServiceHandler, StatusCode};
