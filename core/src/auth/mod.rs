//! 安全与鉴权模块。
//!
//! 负责 TLS 1.3 配置、证书材料以及可选 token 元数据鉴权。

mod tls;

pub use tls::{AuthError, TlsConfig, TokenPolicy};
