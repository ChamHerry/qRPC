# qRPC 详细设计说明书（LLD）

| 文档属性 | 内容 |
|---|---|
| 文档名称 | qRPC 详细设计说明书（Low-Level Design） |
| 版本 | v0.1 (Draft) |
| 日期 | 2026-07-23 |
| 状态 | 草案 / 待评审 |
| 关联文档 | 02-HLD、04-协议规范、05-CABI契约 |

> 本文档给出各模块的详细设计：接口签名（Rust 伪代码，中文注释）、数据结构、状态机与关键流程，作为编码实现依据。trait 字段、错误类型等细节在实现期可微调，但职责与边界以本文档为准。

---

## 1. 引言

### 1.1 模块依赖图（内核）

```
        abi (导出 C ABI)
         │ 依赖
    ┌────┴────┐
   conn      call ──► frame
    │         │
    └────┬────┘
       transport (TAL)
      ┌─────┬─────────┐
   quinn  inproc   (msquic/rdma/tcp 后续)
```

依赖方向自上而下；`frame` 无下游依赖；`transport` 依赖具体后端。

---

## 2. 传输抽象层 `transport`（TAL）

### 2.1 设计目标
抽象"可靠 stream + 不可靠 message"，使上层 `call` 不感知具体传输（quinn/MsQuic/RDMA/TCP）。

### 2.2 核心 trait

```rust
// 传输后端 trait:每个传输族(quinn/inproc/msquic/rdma)实现此接口
pub trait Transport: Send + Sync {
    // 作为客户端发起连接
    async fn connect(&self, addr: &SocketAddr, cfg: &TlsConfig) -> Result<Arc<dyn Connection>, ConnError>;
    // 作为服务端监听
    async fn listen(&self, addr: &SocketAddr, cfg: &TlsConfig) -> Result<Box<dyn Listener>, ConnError>;
}

// 监听器:接受入站连接
pub trait Listener: Send + Sync {
    async fn accept(&mut self) -> Result<Arc<dyn Connection>, ConnError>;
}

// 一条 QUIC/传输连接:承载多个并发 RPC(多路复用)
pub trait Connection: Send + Sync {
    // 打开一条双向流(每个 RPC 一条)
    async fn open_bi_stream(&self) -> Result<Box<dyn BiStream>, ConnError>;
    // 接受对端打开的流(server 侧)
    async fn accept_bi_stream(&self) -> Result<Box<dyn BiStream>, ConnError>;
    // 发送可选的 datagram(0.1 不用,预留)
    async fn send_datagram(&self, _data: &[u8]) -> Result<(), ConnError> { unimplemented!() }
    // 关闭连接
    async fn close(&self, reason: u32);
}

// 双向流:承载一个 RPC 的可靠字节通道(受 QUIC 流控)
pub trait BiStream: Send + Sync {
    async fn send(&mut self, data: &[u8]) -> Result<(), StreamError>; // 背压时挂起
    async fn recv(&mut self) -> Result<Option<Vec<u8>>, StreamError>; // None=对端半关闭
    fn reset(&mut self, code: u32);       // 取消/复位
    fn is_finished(&self) -> bool;
}
```

### 2.3 后端实现
- `QuinnTransport`：封装 `quinn::Endpoint`，`BiStream` 包装 `quinn::SendStream/RecvStream`。
- `InprocTransport`：内存通道（flume），用于测试与 in-proc 降级；无序列化、零网络。

---

## 3. 帧编解码 `frame`

### 3.1 数据结构

```rust
// 帧类型(对应协议规范 §4)
#[repr(u8)]
pub enum FrameType {
    Metadata = 0x01,
    Data     = 0x02,
    Status   = 0x03,
    Settings = 0x04,
    Rst      = 0x05,
    GoAway   = 0x06,
}

// 帧标志位(对应协议规范 §3.2)
pub mod flags {
    pub const END_STREAM: u8   = 0x01;
    pub const COMPRESSED: u8   = 0x02;
    pub const END_METADATA: u8 = 0x04;
}

// 一个帧:类型 + 标志 + 负载
pub struct Frame {
    pub ty: FrameType,
    pub flags: u8,
    pub payload: Bytes,
}
```

### 3.2 编解码接口

```rust
impl Frame {
    // 编码为字节(8 字节头 + payload)
    pub fn encode(&self) -> Bytes;
    // 从字节流解码一帧
    pub fn decode(buf: &mut BytesMut) -> Result<Option<Frame>, FrameError>;
}
```

### 3.3 高层事件（frame ↔ call 的契约）
`frame` 还提供逻辑事件转换：

```rust
// 将字节流解析为调用级事件,供 call 消费
pub enum CallEvent {
    Metadata(HashMap<String, Bytes>),
    Data(Bytes),
    Status(Status),
}
pub fn bytes_to_event(frame: Frame) -> Result<CallEvent, ProtoError>;
```

---

## 4. 调用状态机 `call`

### 4.1 状态机

```
        ┌──────────┐  发送 METADATA   ┌───────────────┐
 INIT ──┤          ├────────────────►│ METADATA_SENT │
        └──────────┘                  └───────┬───────┘
                                              │ 进入双向交互
                                              ▼
                                        ┌──────────┐
                                  ┌────►│   OPEN   │◄────┐
                                  │     └────┬─────┘     │
                    本端结束发送 ───┤          │           ├── 对端结束
                                  │   ┌──────▼──────┐     │
                                  │   │ HALF_LOCAL  │     │
                                  │   └──────┬──────┘     │
                                  └──────────┴────────────┘
                                              │ 双方结束 / 错误 / 取消
                                              ▼
                                        ┌──────────┐
                                        │  CLOSED  │
                                        └──────────┘
```

### 4.2 接口

```rust
// 一个 RPC 调用(对应一条 BiStream)
pub struct Call { /* 状态、stream、元数据、缓冲 */ }

impl Call {
    // 创建调用,绑定一条双向流
    pub fn new(stream: Box<dyn BiStream>, role: Role) -> Self;
    // 发送元数据(client 起始 / server 回传)
    pub async fn send_metadata(&mut self, meta: HashMap<String, Bytes>) -> Result<(), CallError>;
    // 发送一个消息(序列化后的字节)
    pub async fn send_message(&mut self, data: Bytes, end: bool) -> Result<(), CallError>;
    // 接收下一个事件(消息/状态/结束)
    pub async fn recv_event(&mut self) -> Result<CallEvent, CallError>;
    // 结束并发送 STATUS
    pub async fn finish(&mut self, status: Status) -> Result<(), CallError>;
    // 取消(复位流)
    pub fn cancel(&mut self, code: ErrorCode);
    pub fn state(&self) -> CallState;
}
```

### 4.3 关键流程
- **发送**：`send_message` → 组 DATA 帧 → `stream.send`（背压时 await 挂起）。
- **接收**：`recv_event` → `stream.recv` → `Frame::decode` → 转 `CallEvent`。
- **取消**：`cancel` → `stream.reset`；状态机转 CLOSED，通过事件回调通知绑定层。

---

## 5. 方法路由 `dispatch`（server 侧）

```rust
// 服务端方法分发器
pub struct Dispatcher {
    // 方法全名 -> 处理器
    handlers: HashMap<String, Arc<dyn ServiceHandler>>,
}

// 服务处理器 trait:由 protoc 生成的 server stub 实现
#[async_trait]
pub trait ServiceHandler: Send + Sync {
    // 处理一个调用:从 call 读请求、写响应、回 STATUS
    async fn handle(&self, call: Call, method: &str, meta: HashMap<String, Bytes>) -> Status;
}

impl Dispatcher {
    pub fn register(&mut self, method: &str, handler: Arc<dyn ServiceHandler>);
    // 根据 :method 路由
    pub async fn dispatch(&self, call: Call, meta: HashMap<String, Bytes>) -> Status;
}
```

- 未知方法 → `UNIMPLEMENTED`。
- 处理 panic → 捕获转 `INTERNAL`（不崩溃内核）。

---

## 6. 错误与状态 `error`

```rust
// 状态码(对齐 gRPC,见协议规范 §7)
#[repr(u8)]
pub enum ErrorCode { Ok=0, Cancelled=1, /* ... */ Unauthenticated=16 }

// 调用结束状态
pub struct Status {
    pub code: ErrorCode,
    pub message: String,
    pub details: Option<Bytes>,  // 可选 protobuf 编码详情
}
```

- `to_frame()` → STATUS 帧；`from_event()` 从 CallEvent 构造。

---

## 7. 背压 `flow`

- `flow` 非独立重型组件，而是**贯穿 `call` ↔ `transport` 的约定**：
  - `BiStream::send` 在 QUIC 流控阻塞时返回 `Pending`（Rust async 挂起）。
  - `call.send_message` 透传此挂起 → 绑定层事件回调 `QR_EV_WRITABLE` → 用户发送循环停住。
- 接收侧：用户不读 → QUIC 接收窗口耗尽 → 自动 backpressure 对端。

---

## 8. 连接管理 `conn`

```rust
// 连接池:管理到同一对端的连接复用
pub struct ConnectionPool { /* addr -> Arc<Connection> */ }

impl ConnectionPool {
    pub async fn dial(&self, addr: &SocketAddr, cfg: &TlsConfig) -> Result<Arc<dyn Connection>, ConnError>;
    // 心跳(keepalive):定期 PING,超时判定断连
    pub fn enable_keepalive(&self, interval: Duration, timeout: Duration);
    // 断连检测:连接失效时通知所有进行中 Call -> UNAVAILABLE
}
```

---

## 9. C ABI 导出 `abi`

见 `05-接口规范-CABI契约.md` 完整清单。`abi` 组件职责：
- 把 `Call/Connection/Server` 包装为不透明句柄 `qr_call_t*/qr_conn_t*/qr_server_t*`。
- 暴露同步非阻塞操作 + 全局事件回调。
- 处理 `tokio` runtime 调度（内核 async → C ABI 同步边界）。

```rust
// 关键:全局事件回调注册(见 CABI 契约)
pub fn qr_set_event_callback(cb: Option<qr_event_cb>);
// 每个 Call 状态变化时,在 runtime 线程触发回调
```

---

## 10. 绑定层详细设计

### 10.1 Rust 绑定（`qrpc-rs`，直连内核）
- 直接依赖内核 crate，暴露 `async fn` API。
- Client：生成的 trait，方法为 `async fn get_user(&self, req: GetUserReq) -> Result<GetUserResp, Status>`。
- Server：生成的 trait，用户实现 handler。
- 背压：原生 `await`。

### 10.2 Python 绑定（`qrpc-python`，PyO3）
- 模块包装内核（PyO3 直接链接 Rust crate 或经 C ABI）。
- 暴露 `asyncio` 协程 API。
- 事件回调桥接：`QR_EV_*` → `loop.call_soon_threadsafe(future.set_result/wakeup)`。
- 用户对象 ↔ protobuf 字节：用 Python `protobuf` 库。

### 10.3 Go 绑定（`qrpc-go`，cgo）
- cgo 包装 C ABI。
- 暴露阻塞式 + `context.Context`（超时/取消）+ `channel`（streaming）。
- 事件回调 → 写 channel → goroutine 阻塞读。
- 用 `google.golang.org/protobuf` 做序列化。

---

## 11. 代码生成器 `protoc-gen-qrpc`

### 11.1 输入/输出
- 输入：`.proto` 的 `service` 定义。
- 输出：目标语言的 client trait/class + server trait/interface。
- 消息类：委托 protoc 原生插件（不重复实现）。

### 11.2 生成方法签名（按模式）

| 模式 | client 方法签名（Rust 示例） |
|---|---|
| unary | `async fn m(&self, req: Req) -> Result<Resp, Status>` |
| server stream | `async fn m(&self, req: Req) -> Result<ServerStream<Resp>, Status>` |
| client stream | `async fn m(&self, s: ClientStream<Req>) -> Result<Resp, Status>` |
| bidi stream | `async fn m(&self, s: BidiStream<Req, Resp>) -> Result<(), Status>` |

### 11.3 实现语言
- 用 Rust 编写（`prost-build` 或手写 protoc 插件协议处理）。

---

## 12. 异常处理与边界

| 边界 | 处理 |
|---|---|
| 畸形帧 | `FrameError` → 终止 Call，`INTERNAL` |
| handler panic | 捕获 → `INTERNAL`，不崩溃 |
| 连接断开 | 所有 Call → `UNAVAILABLE`，清理资源 |
| 超大消息 | 单个逻辑消息超过默认上限 **4 MiB**（可配置）→ `RESOURCE_EXHAUSTED` |
| C ABI 调用错误 | 返回错误码 + `qr_last_error` 取消息 |

---

## 13. 待实现期细化项
- `Bytes`/`BytesMut` 统一用 `bytes` crate。
- async runtime 统一 tokio。
- 句柄表（`HashMap<HandleId, Arc<Call>>`）的并发与回收策略。
- 代码生成的 metadata 类型（各语言 HashMap/dict/map）。
