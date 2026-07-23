package qrpc

/*
#cgo CFLAGS: -I${SRCDIR}/_cgo
#cgo darwin LDFLAGS: -L${SRCDIR}/../target/debug -lqrpc_core -framework Security -framework CoreFoundation
#cgo linux LDFLAGS: -L${SRCDIR}/../target/debug -lqrpc_core -ldl -lpthread -lm
#include <stdlib.h>
#include "qrpc_ffi.h"
*/
import "C"

import (
	"context"
	"errors"
	"fmt"
	"sync"
	"unsafe"
)

type Client struct {
	addr string
}

type Stream struct {
	ctx    context.Context
	conn   *C.qr_conn_t
	call   *C.qr_call_t
	recvCh chan streamResult
	mu     sync.Mutex
	closed bool
}

type streamResult struct {
	data []byte
	err  error
}

func NewClient(addr string) *Client {
	return &Client{addr: addr}
}

func (c *Client) Unary(ctx context.Context, method string, request []byte) ([]byte, error) {
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	default:
	}

	resultCh := make(chan streamResult, 1)
	go func() {
		stream, err := c.NewStream(context.Background(), method)
		if err != nil {
			resultCh <- streamResult{err: err}
			return
		}
		defer stream.destroy()
		if err := stream.Send(request); err != nil {
			resultCh <- streamResult{err: err}
			return
		}
		data, err := stream.Recv()
		if err != nil {
			resultCh <- streamResult{err: err}
			return
		}
		_ = stream.CloseSend()
		resultCh <- streamResult{data: data}
	}()

	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case result := <-resultCh:
		return result.data, result.err
	}
}

func (c *Client) NewStream(ctx context.Context, method string) (*Stream, error) {
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	default:
	}
	if method == "" {
		return nil, errors.New("method is required")
	}
	if rc := C.qr_init(); rc != C.QR_OK {
		return nil, lastError()
	}

	cAddr := C.CString(c.addr)
	defer C.free(unsafe.Pointer(cAddr))
	var conn *C.qr_conn_t
	if rc := C.qr_connect(cAddr, nil, &conn); rc != C.QR_OK {
		return nil, lastError()
	}

	cMethod := C.CString(method)
	defer C.free(unsafe.Pointer(cMethod))
	var call *C.qr_call_t
	if rc := C.qr_call_begin(conn, cMethod, nil, &call); rc != C.QR_OK {
		C.qr_conn_close(conn)
		return nil, lastError()
	}

	stream := &Stream{
		ctx:    ctx,
		conn:   conn,
		call:   call,
		recvCh: make(chan streamResult, 8),
	}
	return stream, nil
}

func (s *Stream) Send(data []byte) error {
	select {
	case <-s.ctx.Done():
		C.qr_call_cancel(s.call)
		return s.ctx.Err()
	default:
	}
	s.mu.Lock()
	if s.closed {
		s.mu.Unlock()
		return errors.New("stream is closed")
	}
	rc := C.qr_call_send(s.call, bytesPtr(data), C.size_t(len(data)))
	s.mu.Unlock()
	if rc != C.QR_OK {
		return lastError()
	}

	go func() {
		payload, err := recv(s.call)
		s.recvCh <- streamResult{data: payload, err: err}
	}()
	return nil
}

func (s *Stream) Recv() ([]byte, error) {
	select {
	case <-s.ctx.Done():
		C.qr_call_cancel(s.call)
		return nil, s.ctx.Err()
	case result := <-s.recvCh:
		return result.data, result.err
	}
}

func (s *Stream) CloseSend() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return nil
	}
	s.closed = true
	if rc := C.qr_call_send_close(s.call); rc != C.QR_OK {
		return lastError()
	}
	return nil
}

func (s *Stream) destroy() {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.call != nil {
		C.qr_call_destroy(s.call)
		s.call = nil
	}
	if s.conn != nil {
		C.qr_conn_close(s.conn)
		s.conn = nil
	}
	C.qr_shutdown()
}

func recv(call *C.qr_call_t) ([]byte, error) {
	var out *C.uint8_t
	var outLen C.size_t
	if rc := C.qr_call_recv(call, &out, &outLen); rc != C.QR_OK {
		return nil, lastError()
	}
	defer C.qr_buf_free(out)
	return C.GoBytes(unsafe.Pointer(out), C.int(outLen)), nil
}

func bytesPtr(data []byte) *C.uint8_t {
	if len(data) == 0 {
		return nil
	}
	return (*C.uint8_t)(unsafe.Pointer(&data[0]))
}

func lastError() error {
	msg := C.qr_last_error(nil)
	if msg == nil {
		return errors.New("qrpc c abi error")
	}
	return fmt.Errorf("%s", C.GoString(msg))
}
