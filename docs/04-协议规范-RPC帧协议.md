# qRPC 协议规范 —— RPC 帧协议

| 文档属性 | 内容 |
|---|---|
| 文档名称 | qRPC RPC 帧协议规范（Wire Protocol） |
| 版本 | v0.1 (Draft) |
| 日期 | 2026-07-23 |
| 状态 | 草案 / 待评审 |
| 传输层 | QUIC（RFC 9000）+ TLS 1.3（RFC 9001） |
| 关联文档 | 03-LLD、05-CABI契约 |

> 本规范定义 qRPC 在 QUIC stream 之上的应用层帧协议，是内核 `frame` 组件与跨语言互通的**唯一实现依据**。所有整数采用**大端序（big-endian）**，长度字段单位为**字节**。

---

## 1. 设计目标与原则

1. **简洁**：不引入 HTTP 语义层（无 HPACK、无伪头）。
2. **高效**：固定小帧头，便于解析与零拷贝。
3. **流控复用 QUIC**：不自建传输级流控。
4. **模式统一**：四种调用模式共用同一套帧，仅消息数量/方向不同。
5. **可演进**：预留版本协商与帧类型扩展空间。

---

## 2. 传输层使用

### 2.1 连接
- 基于 **QUIC 连接**，强制 TLS 1.3。
- 一条连接承载多个并发 RPC（多路复用）。

### 2.2 Stream 映射
- **每个 RPC 调用 = 一条 QUIC bidirectional stream**。
- Client 发起的 RPC：client 侧打开 stream，stream ID 为 client-initiated bidirectional。
- 调用结束 = 该 stream 关闭（正常 STATUS END_STREAM，或异常 STREAM_RESET）。
- Stream 间独立：一个 RPC 的丢包/错误不阻塞其他 RPC。

### 2.3 Datagram（0.1 不使用）
- QUIC datagram 通道在 0.1 不承载 RPC。
- 0.3 起，可选地将"调用关联的带外低延迟数据"放 datagram（协议扩展，向后兼容）。

---

## 3. 帧格式（Wire Format）

### 3.1 帧头（固定 8 字节）

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|    Type       |    Flags      |          Length               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+               +
|                               |          Reserved             |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                          Payload ...                          |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

| 字段 | 偏移 | 长度 | 说明 |
|---|---|---|---|
| Type | 0 | 1 字节 | 帧类型（见 §4） |
| Flags | 1 | 1 字节 | 标志位（见 §3.2） |
| Length | 2 | 4 字节 | Payload 长度（大端，不含帧头） |
| Reserved | 6 | 2 字节 | 预留，发送时置 0，接收时忽略 |
| Payload | 8 | Length | 由 Type 决定内容 |

### 3.2 Flags 位定义

| 位 | 名称 | 含义 |
|---|---|---|
| 0x01 | END_STREAM | 本帧是所在 stream 的最后一帧；带此位的 STATUS 表示调用终结 |
| 0x02 | COMPRESSED | payload 经过压缩（**0.1 不实现，固定置 0**；预留，未来按需启用） |
| 0x04 | END_METADATA | METADATA 帧集合的结束标志（见 §5.1）|
| 其余 | — | 预留，置 0 |

---

## 4. 帧类型

| Type 值 | 名称 | 方向 | 用途 |
|---|---|---|---|
| 0x01 | METADATA | 双向 | 调用元数据（method、timeout、auth、自定义 header） |
| 0x02 | DATA | 双向 | 一个序列化消息（protobuf 字节） |
| 0x03 | STATUS | 双向 | 调用结束状态（code + message + details），通常带 END_STREAM |
| 0x04 | SETTINGS | 双向 | 连接级设置（**0.1 不实现**；预留，未来用于版本/能力协商） |
| 0x05 | RST | 双向 | 复位（通常用 QUIC STREAM_RESET 代替，预留） |
| 0x06 | GOAWAY | 双向 | 连接级优雅关闭（可选） |

> 实现必须识别 0x01–0x03；遇到未知类型时：若最高位（0x80）为 1 视为可忽略并跳过，否则报 PROTOCOL_ERROR。

---

## 5. 帧类型详解

### 5.1 METADATA 帧（0x01）

承载调用元数据。payload 为若干 key-value 条目（连续存放），结构：

```
每个条目:
  Key-Len    : 2 字节 (大端)
  Key        : Key-Len 字节 (ASCII，小写)
  Value-Len  : 4 字节 (大端)
  Value      : Value-Len 字节 (二进制安全)
```

**预定义键（约定，非强制大小写转换；实现统一用小写）：**

| 键 | 含义 | 示例 |
|---|---|---|
| `:method` | 完整方法名（`/包.服务/方法`） | `/user.UserService/GetUser` |
| `:timeout` | 调用超时（毫秒，十进制 ASCII） | `5000` |
| `:auth` | 鉴权 token | `<token 字节>` |
| `:content-type` | 序列化类型（0.1 固定 `application/protobuf`）| `application/protobuf` |
| `:call-id` | 调用 ID（调试/关联，可选） | `abc123` |
| 其他 | 用户自定义 header | `trace-id: ...` |

> 冒号前缀键为协议保留键；用户自定义键不得以 `:` 开头。一个 stream 的开头由一个或多个 METADATA 帧组成，最后一个带 `END_METADATA` 标志。

### 5.2 DATA 帧（0x02）

payload 即一个序列化消息的字节（0.1 为 protobuf）。
- 一个 DATA 帧承载恰好一个消息。
- 大消息由上层切片为多个 DATA 帧（消息边界靠帧 Length，不靠额外分隔符）。
- **消息大小上限（决议 D5）**：单个逻辑消息（所有 DATA 帧重组后）默认上限 **4 MiB（4 × 1024 × 1024 字节）**，可配置。超过上限 → 拒绝并返回 `RESOURCE_EXHAUSTED`。

### 5.3 STATUS 帧（0x03）

表示调用结束状态。通常带 `END_STREAM`。

payload：
```
Code       : varint       (状态码，见 §7)
Msg-Len    : 4 字节 (大端)
Message    : Msg-Len 字节 (UTF-8，人类可读)
Details-Len: 4 字节 (大端)   (0 表示无)
Details    : Details-Len 字节 (protobuf 编码的结构化详情，可选)
```

- `Code = 0 (OK)` 表示成功。
- 非 0 表示错误（见 §7）。

### 5.4 varint 编码
采用与 protobuf 一致的 varint（每字节低 7 位为有效位，最高位为继续标志），以便与 protobuf 生态工具兼容。

---

## 6. 调用生命周期

### 6.1 Unary（1 请求 → 1 响应）

```
Client                                         Server
  │── METADATA(:method, :timeout, ...) END_METADATA ─►│
  │── DATA(req) ─────────────────────────────────────►│
  │                                                   │ (执行)
  │◄──────────────── DATA(resp) ──────────────────────│
  │◄──────────────── STATUS(OK, END_STREAM) ──────────│
```

### 6.2 Server Streaming（1 请求 → N 响应）

```
Client                                         Server
  │── METADATA END_METADATA ─►│
  │── DATA(req) ─────────────►│
  │◄──── DATA(resp1) ─────────│
  │◄──── DATA(resp2) ─────────│
  │◄──── ...                  │
  │◄──── STATUS(OK, END_STREAM)│
```

### 6.3 Client Streaming（N 请求 → 1 响应）

```
Client                                         Server
  │── METADATA END_METADATA ─►│
  │── DATA(req1) ────────────►│
  │── DATA(req2) ────────────►│
  │── DATA(req_n, END_STREAM 前) ►│  (client 用半关闭或 STATUS 触发收束)
  │◄──── STATUS(OK, END_STREAM)│   (server 汇总后返回)
```
> **Client 结束发送（决议 D3）**：client 发送侧用 stream 半关闭 + 最后一个 DATA 带 `END_STREAM` 表示"上传完毕"；**不接受**单独的空 END_STREAM 帧。

### 6.4 Bidirectional Streaming（N ↔ N）

```
Client                                         Server
  │── METADATA END_METADATA ─►│
  │◄► DATA ... (交替，全双工) ◄►│
  │← STATUS(END_STREAM) / 任一方 reset 结束 →│
```
- 任一方发 `END_STREAM` 的帧表示自己一侧结束；双方都结束后 stream 关闭。

### 6.5 调用状态机（Call Lifecycle）
状态：`INIT → METADATA_SENT → OPEN → HALF_LOCAL / HALF_REMOTE → CLOSED`（详见 LLD `call` 组件）。

---

## 7. 错误与状态码

复用 gRPC 状态码（语义、数值一致），便于迁移：

| 码 | 名称 | 典型触发 |
|---|---|---|
| 0 | OK | 成功 |
| 1 | CANCELLED | 主动取消 |
| 2 | UNKNOWN | 未知错误 |
| 3 | INVALID_ARGUMENT | 参数非法 |
| 4 | DEADLINE_EXCEEDED | 超时 |
| 5 | NOT_FOUND | 资源不存在 |
| 6 | ALREADY_EXISTS | 已存在 |
| 7 | PERMISSION_DENIED | 无权限 |
| 8 | RESOURCE_EXHAUSTED | 资源耗尽（含背压触发） |
| 9 | FAILED_PRECONDITION | 前置条件不满足 |
| 10 | ABORTED | 中止 |
| 11 | OUT_OF_RANGE | 越界 |
| 12 | UNIMPLEMENTED | 方法未实现 |
| 13 | INTERNAL | 内部错误 |
| 14 | UNAVAILABLE | 服务不可用（含连接断开） |
| 15 | DATA_LOSS | 数据丢失 |
| 16 | UNAUTHENTICATED | 未认证 |

**两类错误来源：**
- 应用错误：server handler 返回非 OK → STATUS 帧。
- 系统/传输错误：内核检测（连接断/超时/reset）→ 终止 Call → 映射为 UNAVAILABLE / DEADLINE_EXCEEDED / CANCELLED 等，通过 C ABI 事件通知绑定层。

---

## 8. 流控

- **完全复用 QUIC 原生 per-stream + per-connection 流控**，不自建 WINDOW_UPDATE。
- 当对端不读，QUIC 接收窗口耗尽，发送侧 `call.send_msg` 阻塞（Rust async 挂起）→ 背压上传导至用户代码。
- 不存在协议级额外流控帧。

---

## 9. 安全

- 强制 TLS 1.3（rustls），无明文模式。
- 证书 / CA 可配置。
- 可选应用层 token（`:auth`）。

---

## 10. 版本协商与兼容性

- 0.1 不实现显式版本协商（单版本）。
- 预留：未来在 SETTINGS 帧中协商协议版本、能力（如 datagram、压缩、codec）。
- 未知帧类型按 §4 规则处理（最高位为 1 可忽略），保证前向兼容。

---

## 11. 完整示例（Unary，带自定义 header）

```
Client → Server (stream 上):

帧1  METADATA:
     Flags = END_METADATA (0x04)
     Payload:
       :method = /user.UserService/GetUser
       :timeout = 5000
       :content-type = application/protobuf
       :auth = <token>
       trace-id = abc-123

帧2  DATA:
     Flags = 0x00
     Payload = <protobuf: user_id=42>

Server → Client:

帧3  DATA:
     Payload = <protobuf: {name:"Alice",...}>

帧4  STATUS:
     Flags = END_STREAM (0x01)
     Payload:
       Code = 0 (OK)
       Message = ""
       Details = (空)
```

---

## 12. 设计决议（2026-07-23 评审通过）

| # | 议题 | 决议 |
|---|---|---|
| D1 | 默认端口 | **7000**（可配置） |
| D2 | COMPRESSED 是否进 0.1 | **不进**（固定置 0，预留） |
| D3 | client streaming 结束信号 | **半关闭 + 最后一个 DATA 带 END_STREAM**；不接受空 END_STREAM 帧 |
| D4 | SETTINGS 帧是否进 0.1 | **不进**（单版本，0.1 不做能力协商） |
| D5 | 最大消息默认上限 | **4 MiB**（可配置；超过 → RESOURCE_EXHAUSTED） |

> 以上决议已并入正文（§3.2、§4、§5.2、§6.3），并同步至 `01-SRS §3.3`（默认端口）。
