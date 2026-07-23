//! C ABI 句柄定义。

use crate::call::Metadata;
use crate::error::{ErrorCode, Status};
use std::collections::{HashMap, VecDeque};
use std::ffi::{c_char, c_void, CString};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Mutex;

/// C ABI 返回码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
#[allow(non_camel_case_types)]
pub enum qr_rc {
    QR_ERR = -1,
    QR_OK = 0,
    QR_WOULD_BLOCK = 1,
    QR_CLOSED = 2,
}

#[allow(non_camel_case_types)]
pub type qr_handler_fn = extern "C" fn(*mut qr_call_t, *const c_char, *mut c_void);

#[allow(non_camel_case_types)]
pub struct qr_cfg_t {
    pub tls: Mutex<Option<TlsPaths>>,
    pub keepalive: Mutex<Option<(u32, u32)>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsPaths {
    pub cert_path: String,
    pub key_path: String,
    pub ca_path: String,
}

#[allow(non_camel_case_types)]
pub struct qr_conn_t {
    pub addr: String,
    pub closed: AtomicBool,
    pub user: *mut c_void,
}

#[allow(non_camel_case_types)]
pub struct qr_call_t {
    pub method: CString,
    pub user: *mut c_void,
    pub metadata: Mutex<Metadata>,
    pub timeout_ms: AtomicU64,
    pub inbox: Mutex<VecDeque<Vec<u8>>>,
    pub status: Mutex<Status>,
    pub status_message: Mutex<CString>,
    pub details: Mutex<Vec<u8>>,
}

impl qr_call_t {
    pub fn new(method: CString, user: *mut c_void) -> Self {
        let metadata = vec![(
            ":method".to_string(),
            method.as_bytes().to_vec(),
        )];
        Self {
            method,
            user,
            metadata: Mutex::new(metadata),
            timeout_ms: AtomicU64::new(0),
            inbox: Mutex::new(VecDeque::new()),
            status: Mutex::new(Status::ok()),
            status_message: Mutex::new(CString::new("").expect("empty cstr")),
            details: Mutex::new(Vec::new()),
        }
    }

    pub fn set_status(&self, status: Status) {
        if let Ok(mut details) = self.details.lock() {
            *details = status.details.clone().unwrap_or_default();
        }
        if let Ok(mut message) = self.status_message.lock() {
            *message = CString::new(status.message.clone()).unwrap_or_else(|_| {
                CString::new("status message contains interior nul").expect("static cstr")
            });
        }
        if let Ok(mut stored) = self.status.lock() {
            *stored = status;
        }
    }

    pub fn status_code(&self) -> i32 {
        self.status
            .lock()
            .map(|status| status.code.as_u8() as i32)
            .unwrap_or(ErrorCode::Internal.as_u8() as i32)
    }
}

#[allow(non_camel_case_types)]
pub struct qr_server_t {
    pub addr: String,
    pub services: Mutex<HashMap<String, RegisteredService>>,
    pub closed: AtomicBool,
}

pub struct RegisteredService {
    pub handler: qr_handler_fn,
    pub state: *mut c_void,
}

unsafe impl Send for RegisteredService {}
unsafe impl Sync for RegisteredService {}
