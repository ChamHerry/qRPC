//! 调用状态定义。

/// 调用参与方角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Client,
    Server,
}

/// RPC 调用状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallState {
    Init,
    MetadataSent,
    Open,
    HalfLocal,
    HalfRemote,
    Closed,
    Cancelled,
}
