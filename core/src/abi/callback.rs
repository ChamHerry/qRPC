//! C ABI 全局事件回调。

use super::handles::{qr_call_t, qr_conn_t};
use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};

/// call 级事件类型。
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum qr_event_t {
    QR_EV_READABLE = 1,
    QR_EV_WRITABLE = 2,
    QR_EV_DONE = 3,
    QR_EV_ERROR = 4,
}

/// 连接级事件类型。
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum qr_conn_event_t {
    QR_CONN_EV_CONNECTED = 1,
    QR_CONN_EV_DISCONNECTED = 2,
    QR_CONN_EV_RECONNECTED = 3,
}

#[allow(non_camel_case_types)]
pub type qr_event_cb = extern "C" fn(*mut qr_call_t, qr_event_t, *mut c_void);

#[allow(non_camel_case_types)]
pub type qr_conn_event_cb = extern "C" fn(*mut qr_conn_t, qr_conn_event_t, *mut c_void);

fn call_callback() -> &'static Mutex<Option<qr_event_cb>> {
    static CALLBACK: OnceLock<Mutex<Option<qr_event_cb>>> = OnceLock::new();
    CALLBACK.get_or_init(|| Mutex::new(None))
}

fn conn_callback() -> &'static Mutex<Option<qr_conn_event_cb>> {
    static CALLBACK: OnceLock<Mutex<Option<qr_conn_event_cb>>> = OnceLock::new();
    CALLBACK.get_or_init(|| Mutex::new(None))
}

#[no_mangle]
pub extern "C" fn qr_set_event_callback(cb: Option<qr_event_cb>) {
    if let Ok(mut callback) = call_callback().lock() {
        *callback = cb;
    }
}

#[no_mangle]
pub extern "C" fn qr_set_conn_event_callback(cb: Option<qr_conn_event_cb>) {
    if let Ok(mut callback) = conn_callback().lock() {
        *callback = cb;
    }
}

pub fn trigger_call_event(call: *mut qr_call_t, ev: qr_event_t, user: *mut c_void) {
    if let Ok(callback) = call_callback().lock() {
        if let Some(callback) = *callback {
            callback(call, ev, user);
        }
    }
}

pub fn trigger_conn_event(conn: *mut qr_conn_t, ev: qr_conn_event_t, user: *mut c_void) {
    if let Ok(callback) = conn_callback().lock() {
        if let Some(callback) = *callback {
            callback(conn, ev, user);
        }
    }
}
