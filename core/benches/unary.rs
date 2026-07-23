use criterion::{criterion_group, criterion_main, Criterion};
use qrpc_core::abi::{
    qr_buf_free, qr_call_begin, qr_call_destroy, qr_call_recv, qr_call_send, qr_conn_close,
    qr_connect, qr_init, qr_rc,
};
use std::ffi::CString;
use std::ptr;

fn unary_inproc(c: &mut Criterion) {
    c.bench_function("unary_inproc_64b", |b| {
        b.iter(|| unary_roundtrip_abi(vec![7_u8; 64]))
    });
}

fn unary_roundtrip_abi(payload: Vec<u8>) {
    unsafe {
        assert_eq!(qr_init(), qr_rc::QR_OK);
        let addr = CString::new("127.0.0.1:7000").expect("addr");
        let mut conn = ptr::null_mut();
        assert_eq!(
            qr_connect(addr.as_ptr(), ptr::null_mut(), &mut conn),
            qr_rc::QR_OK
        );

        let method = CString::new("/bench.Echo/Unary").expect("method");
        let mut call = ptr::null_mut();
        assert_eq!(
            qr_call_begin(conn, method.as_ptr(), ptr::null_mut(), &mut call),
            qr_rc::QR_OK
        );
        assert_eq!(
            qr_call_send(call, payload.as_ptr(), payload.len()),
            qr_rc::QR_OK
        );

        let mut out = ptr::null_mut();
        let mut out_len = 0usize;
        assert_eq!(qr_call_recv(call, &mut out, &mut out_len), qr_rc::QR_OK);
        qr_buf_free(out);
        qr_call_destroy(call);
        qr_conn_close(conn);
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = unary_inproc
}
criterion_main!(benches);
