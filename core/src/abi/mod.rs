//! C ABI 导出层。
//!
//! 负责把内核对象包装为不透明句柄，并提供 call 级与连接级事件回调。

mod callback;
mod exports;
mod handles;

pub use callback::{
    qr_conn_event_cb, qr_conn_event_t, qr_event_cb, qr_event_t, trigger_call_event,
    trigger_conn_event,
};
pub use exports::*;
pub use handles::{qr_call_t, qr_cfg_t, qr_conn_t, qr_handler_fn, qr_rc, qr_server_t};
