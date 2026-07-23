//! C ABI 导出函数。

use super::callback::{qr_conn_event_t, qr_event_t, trigger_call_event, trigger_conn_event};
use super::handles::{
    qr_call_t, qr_cfg_t, qr_conn_t, qr_handler_fn, qr_rc, qr_server_t, RegisteredService, TlsPaths,
};
use crate::error::{ErrorCode, Status};
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

fn buffers() -> &'static Mutex<HashMap<usize, Box<[u8]>>> {
    static BUFFERS: OnceLock<Mutex<HashMap<usize, Box<[u8]>>>> = OnceLock::new();
    BUFFERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn last_error() -> &'static Mutex<CString> {
    static LAST_ERROR: OnceLock<Mutex<CString>> = OnceLock::new();
    LAST_ERROR.get_or_init(|| Mutex::new(CString::new("").expect("empty cstr")))
}

fn set_last_error(message: impl AsRef<str>) -> qr_rc {
    if let Ok(mut err) = last_error().lock() {
        *err = CString::new(message.as_ref()).unwrap_or_else(|_| {
            CString::new("error message contains interior nul").expect("static cstr")
        });
    }
    qr_rc::QR_ERR
}

unsafe fn cstr_to_string(ptr: *const c_char, name: &str) -> Result<String, qr_rc> {
    if ptr.is_null() {
        set_last_error(format!("{name} is null"));
        return Err(qr_rc::QR_ERR);
    }
    CStr::from_ptr(ptr)
        .to_str()
        .map(|value| value.to_string())
        .map_err(|_| set_last_error(format!("{name} is not valid UTF-8")))
}

#[no_mangle]
pub extern "C" fn qr_init() -> qr_rc {
    qr_rc::QR_OK
}

#[no_mangle]
pub extern "C" fn qr_shutdown() {}

#[no_mangle]
pub extern "C" fn qr_cfg_new() -> *mut qr_cfg_t {
    Box::into_raw(Box::new(qr_cfg_t {
        tls: Mutex::new(None),
        keepalive: Mutex::new(None),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn qr_cfg_free(cfg: *mut qr_cfg_t) {
    if !cfg.is_null() {
        drop(Box::from_raw(cfg));
    }
}

#[no_mangle]
pub unsafe extern "C" fn qr_cfg_set_tls(
    cfg: *mut qr_cfg_t,
    cert_path: *const c_char,
    key_path: *const c_char,
    ca_path: *const c_char,
) -> qr_rc {
    if cfg.is_null() {
        return set_last_error("cfg is null");
    }
    let cert_path = match cstr_to_string(cert_path, "cert_path") {
        Ok(value) => value,
        Err(rc) => return rc,
    };
    let key_path = match cstr_to_string(key_path, "key_path") {
        Ok(value) => value,
        Err(rc) => return rc,
    };
    let ca_path = match cstr_to_string(ca_path, "ca_path") {
        Ok(value) => value,
        Err(rc) => return rc,
    };
    let cfg = &*cfg;
    match cfg.tls.lock() {
        Ok(mut tls) => {
            *tls = Some(TlsPaths {
                cert_path,
                key_path,
                ca_path,
            });
            qr_rc::QR_OK
        }
        Err(_) => set_last_error("cfg tls lock poisoned"),
    }
}

#[no_mangle]
pub unsafe extern "C" fn qr_cfg_set_keepalive(
    cfg: *mut qr_cfg_t,
    interval_ms: u32,
    timeout_ms: u32,
) -> qr_rc {
    if cfg.is_null() {
        return set_last_error("cfg is null");
    }
    let cfg = &*cfg;
    match cfg.keepalive.lock() {
        Ok(mut keepalive) => {
            *keepalive = Some((interval_ms, timeout_ms));
            qr_rc::QR_OK
        }
        Err(_) => set_last_error("cfg keepalive lock poisoned"),
    }
}

#[no_mangle]
pub unsafe extern "C" fn qr_connect(
    addr: *const c_char,
    _cfg: *mut qr_cfg_t,
    out_conn: *mut *mut qr_conn_t,
) -> qr_rc {
    if out_conn.is_null() {
        return set_last_error("out_conn is null");
    }
    let addr = match cstr_to_string(addr, "addr") {
        Ok(value) => value,
        Err(rc) => return rc,
    };
    let conn = Box::new(qr_conn_t {
        addr,
        closed: AtomicBool::new(false),
        user: ptr::null_mut(),
    });
    let conn = Box::into_raw(conn);
    *out_conn = conn;
    trigger_conn_event(conn, qr_conn_event_t::QR_CONN_EV_CONNECTED, ptr::null_mut());
    qr_rc::QR_OK
}

#[no_mangle]
pub unsafe extern "C" fn qr_conn_close(conn: *mut qr_conn_t) {
    if conn.is_null() {
        return;
    }
    (*conn).closed.store(true, Ordering::SeqCst);
    trigger_conn_event(
        conn,
        qr_conn_event_t::QR_CONN_EV_DISCONNECTED,
        (*conn).user,
    );
    drop(Box::from_raw(conn));
}

#[no_mangle]
pub unsafe extern "C" fn qr_call_begin(
    conn: *mut qr_conn_t,
    method: *const c_char,
    user: *mut c_void,
    out_call: *mut *mut qr_call_t,
) -> qr_rc {
    if conn.is_null() || out_call.is_null() {
        return set_last_error("conn or out_call is null");
    }
    if (*conn).closed.load(Ordering::SeqCst) {
        return set_last_error("connection is closed");
    }
    let method = match cstr_to_string(method, "method") {
        Ok(value) => value,
        Err(rc) => return rc,
    };
    let method = match CString::new(method) {
        Ok(method) => method,
        Err(_) => return set_last_error("method contains interior nul"),
    };
    let call = Box::into_raw(Box::new(qr_call_t::new(method, user)));
    *out_call = call;
    qr_rc::QR_OK
}

#[no_mangle]
pub unsafe extern "C" fn qr_call_set_metadata(
    call: *mut qr_call_t,
    keys: *const *const c_char,
    vals: *const *const c_char,
    val_lens: *const usize,
    n: usize,
) -> qr_rc {
    if call.is_null() {
        return set_last_error("call is null");
    }
    if n > 0 && (keys.is_null() || vals.is_null() || val_lens.is_null()) {
        return set_last_error("metadata arrays are null");
    }
    let mut metadata = Vec::new();
    for i in 0..n {
        let key_ptr = *keys.add(i);
        let val_ptr = *vals.add(i);
        let val_len = *val_lens.add(i);
        let key = match cstr_to_string(key_ptr, "metadata key") {
            Ok(value) => value,
            Err(rc) => return rc,
        };
        if val_ptr.is_null() {
            return set_last_error("metadata value is null");
        }
        let value = std::slice::from_raw_parts(val_ptr as *const u8, val_len).to_vec();
        metadata.push((key, value));
    }
    match (*call).metadata.lock() {
        Ok(mut stored) => {
            *stored = metadata;
            qr_rc::QR_OK
        }
        Err(_) => set_last_error("call metadata lock poisoned"),
    }
}

#[no_mangle]
pub unsafe extern "C" fn qr_call_set_timeout(call: *mut qr_call_t, timeout_ms: u64) -> qr_rc {
    if call.is_null() {
        return set_last_error("call is null");
    }
    (*call).timeout_ms.store(timeout_ms, Ordering::SeqCst);
    qr_rc::QR_OK
}

#[no_mangle]
pub unsafe extern "C" fn qr_call_send(
    call: *mut qr_call_t,
    data: *const u8,
    len: usize,
) -> qr_rc {
    if call.is_null() {
        return set_last_error("call is null");
    }
    if len > 0 && data.is_null() {
        return set_last_error("data is null");
    }
    let bytes = std::slice::from_raw_parts(data, len).to_vec();
    match (*call).inbox.lock() {
        Ok(mut inbox) => inbox.push_back(bytes),
        Err(_) => return set_last_error("call inbox lock poisoned"),
    }
    trigger_call_event(call, qr_event_t::QR_EV_READABLE, (*call).user);
    qr_rc::QR_OK
}

#[no_mangle]
pub unsafe extern "C" fn qr_call_send_close(call: *mut qr_call_t) -> qr_rc {
    if call.is_null() {
        return set_last_error("call is null");
    }
    (*call).set_status(Status::ok());
    trigger_call_event(call, qr_event_t::QR_EV_DONE, (*call).user);
    qr_rc::QR_OK
}

#[no_mangle]
pub unsafe extern "C" fn qr_call_recv(
    call: *mut qr_call_t,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> qr_rc {
    if call.is_null() || out.is_null() || out_len.is_null() {
        return set_last_error("call/out/out_len is null");
    }
    let Some(bytes) = (*call).inbox.lock().ok().and_then(|mut inbox| inbox.pop_front()) else {
        return qr_rc::QR_WOULD_BLOCK;
    };
    let mut boxed = bytes.into_boxed_slice();
    let ptr = boxed.as_mut_ptr();
    let len = boxed.len();
    match buffers().lock() {
        Ok(mut buffers) => {
            buffers.insert(ptr as usize, boxed);
            *out = ptr;
            *out_len = len;
            qr_rc::QR_OK
        }
        Err(_) => set_last_error("buffer registry lock poisoned"),
    }
}

#[no_mangle]
pub unsafe extern "C" fn qr_call_status(
    call: *mut qr_call_t,
    out_code: *mut i32,
    out_message: *mut *const c_char,
    out_details: *mut *const u8,
    out_details_len: *mut usize,
) -> qr_rc {
    if call.is_null()
        || out_code.is_null()
        || out_message.is_null()
        || out_details.is_null()
        || out_details_len.is_null()
    {
        return set_last_error("status output pointer is null");
    }
    let status = match (*call).status.lock() {
        Ok(status) => status.clone(),
        Err(_) => return set_last_error("call status lock poisoned"),
    };
    *out_code = status.code.as_u8() as i32;
    let message = match (*call).status_message.lock() {
        Ok(message) => message,
        Err(_) => return set_last_error("call status message lock poisoned"),
    };
    *out_message = message.as_ptr();
    let details = match (*call).details.lock() {
        Ok(details) => details,
        Err(_) => return set_last_error("call details lock poisoned"),
    };
    *out_details = if details.is_empty() {
        ptr::null()
    } else {
        details.as_ptr()
    };
    *out_details_len = details.len();
    qr_rc::QR_OK
}

#[no_mangle]
pub unsafe extern "C" fn qr_call_user(call: *mut qr_call_t) -> *mut c_void {
    if call.is_null() {
        return ptr::null_mut();
    }
    (*call).user
}

#[no_mangle]
pub unsafe extern "C" fn qr_call_cancel(call: *mut qr_call_t) {
    if call.is_null() {
        return;
    }
    (*call).set_status(Status::new(ErrorCode::Cancelled, "cancelled"));
    trigger_call_event(call, qr_event_t::QR_EV_ERROR, (*call).user);
}

#[no_mangle]
pub unsafe extern "C" fn qr_call_destroy(call: *mut qr_call_t) {
    if !call.is_null() {
        drop(Box::from_raw(call));
    }
}

#[no_mangle]
pub unsafe extern "C" fn qr_server_listen(
    addr: *const c_char,
    _cfg: *mut qr_cfg_t,
    out_srv: *mut *mut qr_server_t,
) -> qr_rc {
    if out_srv.is_null() {
        return set_last_error("out_srv is null");
    }
    let addr = match cstr_to_string(addr, "addr") {
        Ok(value) => value,
        Err(rc) => return rc,
    };
    *out_srv = Box::into_raw(Box::new(qr_server_t {
        addr,
        services: Mutex::new(HashMap::new()),
        closed: AtomicBool::new(false),
    }));
    qr_rc::QR_OK
}

#[no_mangle]
pub unsafe extern "C" fn qr_server_close(srv: *mut qr_server_t) {
    if !srv.is_null() {
        (*srv).closed.store(true, Ordering::SeqCst);
        drop(Box::from_raw(srv));
    }
}

#[no_mangle]
pub unsafe extern "C" fn qr_server_register_service(
    srv: *mut qr_server_t,
    service: *const c_char,
    handler: Option<qr_handler_fn>,
    handler_state: *mut c_void,
) -> qr_rc {
    if srv.is_null() {
        return set_last_error("server is null");
    }
    let Some(handler) = handler else {
        return set_last_error("handler is null");
    };
    let service = match cstr_to_string(service, "service") {
        Ok(value) => value,
        Err(rc) => return rc,
    };
    match (*srv).services.lock() {
        Ok(mut services) => {
            services.insert(
                service,
                RegisteredService {
                    handler,
                    state: handler_state,
                },
            );
            qr_rc::QR_OK
        }
        Err(_) => set_last_error("server services lock poisoned"),
    }
}

#[no_mangle]
pub unsafe extern "C" fn qr_buf_free(buf: *mut u8) {
    if buf.is_null() {
        return;
    }
    if let Ok(mut buffers) = buffers().lock() {
        buffers.remove(&(buf as usize));
    }
}

#[no_mangle]
pub unsafe extern "C" fn qr_last_error(_h: *mut c_void) -> *const c_char {
    last_error()
        .lock()
        .map(|err| err.as_ptr())
        .unwrap_or_else(|_| c"last error lock poisoned".as_ptr())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::callback::{
        qr_set_conn_event_callback, qr_set_event_callback, qr_conn_event_t, qr_event_t,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());
    static CALL_EVENTS: AtomicUsize = AtomicUsize::new(0);
    static CONN_EVENTS: AtomicUsize = AtomicUsize::new(0);

    extern "C" fn call_cb(_call: *mut qr_call_t, _ev: qr_event_t, _user: *mut c_void) {
        CALL_EVENTS.fetch_add(1, Ordering::SeqCst);
    }

    extern "C" fn conn_cb(_conn: *mut qr_conn_t, _ev: qr_conn_event_t, _user: *mut c_void) {
        CONN_EVENTS.fetch_add(1, Ordering::SeqCst);
    }

    extern "C" fn handler(_call: *mut qr_call_t, _method: *const c_char, _state: *mut c_void) {}

    #[test]
    fn abi_config_lifecycle_and_setters_work() {
        let _guard = TEST_LOCK.lock().expect("lock");
        unsafe {
            let cfg = qr_cfg_new();
            assert!(!cfg.is_null());
            assert_eq!(qr_cfg_set_keepalive(cfg, 1000, 500), qr_rc::QR_OK);
            assert_eq!(
                qr_cfg_set_tls(
                    cfg,
                    CString::new("cert.pem").unwrap().as_ptr(),
                    CString::new("key.pem").unwrap().as_ptr(),
                    CString::new("ca.pem").unwrap().as_ptr(),
                ),
                qr_rc::QR_OK
            );
            qr_cfg_free(cfg);
        }
    }

    #[test]
    fn abi_call_send_recv_status_and_callbacks_work() {
        let _guard = TEST_LOCK.lock().expect("lock");
        CALL_EVENTS.store(0, Ordering::SeqCst);
        CONN_EVENTS.store(0, Ordering::SeqCst);
        qr_set_event_callback(Some(call_cb));
        qr_set_conn_event_callback(Some(conn_cb));

        unsafe {
            assert_eq!(qr_init(), qr_rc::QR_OK);
            let mut conn = ptr::null_mut();
            assert_eq!(
                qr_connect(CString::new("127.0.0.1:7000").unwrap().as_ptr(), ptr::null_mut(), &mut conn),
                qr_rc::QR_OK
            );
            assert!(!conn.is_null());
            assert_eq!(CONN_EVENTS.load(Ordering::SeqCst), 1);

            let mut call = ptr::null_mut();
            let user = 0x1234usize as *mut c_void;
            assert_eq!(
                qr_call_begin(conn, CString::new("/demo.Echo/Unary").unwrap().as_ptr(), user, &mut call),
                qr_rc::QR_OK
            );
            assert_eq!(qr_call_user(call), user);

            let payload = b"hello";
            assert_eq!(qr_call_send(call, payload.as_ptr(), payload.len()), qr_rc::QR_OK);
            assert_eq!(CALL_EVENTS.load(Ordering::SeqCst), 1);

            let mut out = ptr::null_mut();
            let mut out_len = 0usize;
            assert_eq!(qr_call_recv(call, &mut out, &mut out_len), qr_rc::QR_OK);
            assert_eq!(std::slice::from_raw_parts(out, out_len), payload);
            qr_buf_free(out);

            assert_eq!(qr_call_send_close(call), qr_rc::QR_OK);
            let mut code = -1;
            let mut message = ptr::null();
            let mut details = ptr::null();
            let mut details_len = 99usize;
            assert_eq!(
                qr_call_status(call, &mut code, &mut message, &mut details, &mut details_len),
                qr_rc::QR_OK
            );
            assert_eq!(code, 0);
            assert!(!message.is_null());
            assert!(details.is_null());
            assert_eq!(details_len, 0);

            qr_call_cancel(call);
            assert!(CALL_EVENTS.load(Ordering::SeqCst) >= 3);
            qr_call_destroy(call);
            qr_conn_close(conn);
            qr_shutdown();
        }

        qr_set_event_callback(None);
        qr_set_conn_event_callback(None);
    }

    #[test]
    fn abi_server_registers_service_handler() {
        let _guard = TEST_LOCK.lock().expect("lock");
        unsafe {
            let mut srv = ptr::null_mut();
            assert_eq!(
                qr_server_listen(
                    CString::new("127.0.0.1:7000").unwrap().as_ptr(),
                    ptr::null_mut(),
                    &mut srv,
                ),
                qr_rc::QR_OK
            );
            assert_eq!(
                qr_server_register_service(
                    srv,
                    CString::new("/demo.Echo").unwrap().as_ptr(),
                    Some(handler),
                    ptr::null_mut(),
                ),
                qr_rc::QR_OK
            );
            assert_eq!((*srv).services.lock().expect("services").len(), 1);
            qr_server_close(srv);
        }
    }
}
