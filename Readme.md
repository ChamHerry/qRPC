# qRPC

qRPC is a QUIC-native, cross-language RPC framework in early design and implementation.

The project targets a Rust core, a stable C ABI for language bindings, and first-class Rust, Python, and Go support. The initial transport backend is planned around QUIC, with a transport abstraction layer reserved for future backends such as MsQuic, RDMA, and TCP fallback.

## Goals

- Native QUIC transport for RPC calls.
- Cross-language interoperability through a stable core contract.
- Protobuf-based IDL and generated client/server stubs.
- Four RPC modes: unary, client streaming, server streaming, and bidirectional streaming.
- Consistent timeout, cancellation, error, backpressure, and TLS behavior across languages.

## Status

This repository is at the initial project setup stage. Implementation code has not been added yet.

## Planned Layout

```text
core/              Rust RPC core
rust/              Rust binding
python/            Python binding
go/                Go binding
protoc-gen-qrpc/   protoc plugin
tests/             Cross-language tests
examples/          Runnable examples
```

## License

TBD.
