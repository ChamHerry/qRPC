#ifndef QRPC_FFI_H
#define QRPC_FFI_H

#include <stdint.h>
#include <stddef.h>

typedef enum {
    QR_ERR = -1,
    QR_OK = 0,
    QR_WOULD_BLOCK = 1,
    QR_CLOSED = 2,
} qr_rc;

typedef struct qr_cfg_t qr_cfg_t;
typedef struct qr_conn_t qr_conn_t;
typedef struct qr_call_t qr_call_t;
typedef struct qr_server_t qr_server_t;

qr_rc qr_init(void);
void qr_shutdown(void);

qr_rc qr_connect(const char* addr, qr_cfg_t* cfg, qr_conn_t** out_conn);
void qr_conn_close(qr_conn_t* conn);

qr_rc qr_call_begin(qr_conn_t* conn, const char* method, void* user, qr_call_t** out_call);
qr_rc qr_call_send(qr_call_t* call, const uint8_t* data, size_t len);
qr_rc qr_call_recv(qr_call_t* call, uint8_t** out, size_t* out_len);
qr_rc qr_call_send_close(qr_call_t* call);
void qr_call_cancel(qr_call_t* call);
void qr_call_destroy(qr_call_t* call);

void qr_buf_free(uint8_t* buf);
const char* qr_last_error(void* h);

#endif
