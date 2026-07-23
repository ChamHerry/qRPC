# qRPC 接口规范 —— C ABI 契约

| 文档属性 | 内容 |
|---|---|
| 文档名称 | qRPC C ABI 契约（Cross-Language Interface） |
| 版本 | v0.1 (Draft) |
| 日期 | 2026-07-23 |
| 状态 | 草案 / 待评审（1.0 冻结前可演进，冻结后保证向后兼容） |
| 关联文档 | 03-LLD、04-协议规范 |

> 本契约是所有非 Rust 语言绑定（Python / Go / …）调用 qRPC 内核的**唯一稳定接口**。Rust 绑定绕过本契约直连内核。本文档是绑定层实现与一致性测试的依据。

---

## 1. 设计原则

1. **稳定最小**：只暴露必要的句柄与操作；内部迭代不破坏 ABI。
2. **句柄式**：所有资源（连接/调用/服务器）以不透明指针（`qr_*_t*`）表示。
3. **同步非阻塞 + 事件回调**：C 侧操作非阻塞（返回就绪/阻塞/错误），异步就绪通过**全局单一事件回调**通知。
4. **明确的内存所有权**：每个 buffer 的分配/释放责任清晰（见 §5）。
5. **不直接暴露 Rust 类型**：边界只出现 C 类型（指针、整型、字节缓冲）。

---

## 2. 句柄模型

```c
typedef struct qr_conn_s   qr_conn_t;    /* 连接 */
typedef struct qr_call_s   qr_call_t;    /* 一个 RPC 调用 */
typedef struct qr_server_s qr_server_t;  /* 服务端监听器 */
typedef struct qr_cfg_s    qr_cfg_t;     /* 配置(TLS 等) */
```

- 句柄由内核创建，**调用方不得直接解引用或释放其内存**，必须用对应的 `qr_*_close/destroy` 释放。
- 句柄在多线程间共享的安全性见 §6。

---

## 3. 异步事件模型（核心）

内核内部是 Rust async；C 侧无法直接 await。采用**全局单一事件回调 + readiness 查询**：

```c
/* 事件类型 */
typedef enum {
    QR_EV_READABLE = 1,  /* 有消息可读(收到 DATA)        */
    QR_EV_WRITABLE = 2,  /* 流控放开,可继续 send         */
    QR_EV_DONE     = 3,  /* 收到 STATUS,调用结束(成功)   */
    QR_EV_ERROR    = 4,  /* 出错(连接断/超时/reset)       */
} qr_event_t;

/* 全局事件回调(绑定层启动时注册一次)
 * call   : 发生事件的调用句柄
 * ev     : 事件类型
 * user   : 创建该 call 时传入的用户数据指针(见 §4.3) */
typedef void (*qr_event_cb)(qr_call_t* call, qr_event_t ev, void* user);

/* 注册/注销全局事件回调 */
void qr_set_event_callback(qr_event_cb cb);
```

**为什么是"全局单一回调"而非每操作注册回调：** 把跨语言回调数量降到 1，大幅降低 FFI 回调的生命周期与线程安全管理复杂度；绑定层用 `call` 句柄 + 关联的 `user` 数据分发到对应语言对象。

### 3.1 连接级事件回调（与 call 级分离，决议 D3）

连接级事件（连接建立/断开/重连）独立于 call 级事件，使用单独的回调，避免与每个调用的 `QR_EV_*` 混淆：

```c
/* 连接级事件类型 */
typedef enum {
    QR_CONN_EV_CONNECTED    = 1,  /* 连接已建立(含 0-RTT 完成) */
    QR_CONN_EV_DISCONNECTED = 2,  /* 连接断开 */
    QR_CONN_EV_RECONNECTED  = 3,  /* (未来)重连成功 */
} qr_conn_event_t;

/* 全局连接级事件回调(绑定层启动时注册一次) */
typedef void (*qr_conn_event_cb)(qr_conn_t* conn, qr_conn_event_t ev, void* user);

/* 注册/注销连接级事件回调 */
void qr_set_conn_event_callback(qr_conn_event_cb cb);
```

- 连接断开时：先触发 `QR_CONN_EV_DISCONNECTED`，随后该连接上每个进行中的 call 触发 `QR_EV_ERROR`（映射 `UNAVAILABLE`）。
- 绑定层据此统一回收连接对象与相关资源。

---

## 4. 函数清单

### 4.1 结果码

```c
typedef enum {
    QR_OK          =  0,
    QR_WOULD_BLOCK =  1,   /* 非阻塞操作暂未就绪(背压) */
    QR_CLOSED      =  2,   /* 对端半关闭 / 调用已结束   */
    QR_ERR         = -1,   /* 一般错误,查 qr_last_error */
} qr_rc;
```

### 4.2 配置与初始化

```c
qr_cfg_t* qr_cfg_new(void);            /* 新建配置 */
void      qr_cfg_free(qr_cfg_t* cfg);

/* 设置 TLS 证书 / 私钥 / CA */
qr_rc qr_cfg_set_tls(qr_cfg_t* cfg,
                     const char* cert_path,
                     const char* key_path,
                     const char* ca_path);

/* keepalive */
qr_rc qr_cfg_set_keepalive(qr_cfg_t* cfg, uint32_t interval_ms, uint32_t timeout_ms);

/* 初始化内核(启动 tokio runtime),进程内调用一次 */
qr_rc qr_init(void);
void  qr_shutdown(void);
```

### 4.3 客户端调用

```c
/* 发起连接 */
qr_rc qr_connect(const char* addr,          /* "host:port" */
                 qr_cfg_t* cfg,
                 qr_conn_t** out_conn);

/* 关闭连接 */
void qr_conn_close(qr_conn_t* conn);

/* 发起一个调用,返回 call 句柄
 * method : "/包.服务/方法"
 * user   : 绑定层用于关联语言对象(随事件回调返回) */
qr_rc qr_call_begin(qr_conn_t* conn,
                    const char* method,
                    void* user,
                    qr_call_t** out_call);

/* 设置调用元数据(必须在首次 send 前调用)
 * keys/vals/lengths 为平行数组,n 为条目数 */
qr_rc qr_call_set_metadata(qr_call_t* call,
                           const char* const* keys,
                           const char* const* vals,
                           const size_t* val_lens,
                           size_t n);

/* 设置超时(deadline) */
qr_rc qr_call_set_timeout(qr_call_t* call, uint64_t timeout_ms);

/* 非阻塞发送一个消息(序列化后的字节)
 * 返回 QR_OK / QR_WOULD_BLOCK(背压,等 QR_EV_WRITABLE)/ QR_ERR */
qr_rc qr_call_send(qr_call_t* call, const uint8_t* data, size_t len);

/* 标记本端发送结束(client streaming / bidi 用) */
qr_rc qr_call_send_close(qr_call_t* call);

/* 非阻塞接收一个消息
 * 返回 QR_OK(*out 被 kernel 分配,需 qr_buf_free)/ QR_WOULD_BLOCK(等 QR_EV_READABLE)/ QR_CLOSED */
qr_rc qr_call_recv(qr_call_t* call, uint8_t** out, size_t* out_len);

/* 取调用结束状态(收到 QR_EV_DONE / QR_EV_ERROR 后调用) */
qr_rc qr_call_status(qr_call_t* call,
                     int32_t* out_code,
                     const char** out_message,   /* kernel 持有,不释放 */
                     const uint8_t** out_details,
                     size_t* out_details_len);

/* 取与本 call 关联的用户数据 */
void* qr_call_user(qr_call_t* call);

/* 主动取消(复位流) */
void qr_call_cancel(qr_call_t* call);

/* 释放 call 句柄(调用结束后) */
void qr_call_destroy(qr_call_t* call);
```

### 4.4 服务端

```c
/* 启动监听 */
qr_rc qr_server_listen(const char* addr,
                       qr_cfg_t* cfg,
                       qr_server_t** out_srv);
void qr_server_close(qr_server_t* srv);

/* 注册一个服务处理器(绑定层生成的 server stub 设置;决议 D4:按 service 注册)
 * 一个 service handler 负责该 service 下所有方法的分发
 * service : "/包.服务"(如 "/user.UserService")
 * handler : 处理函数指针(内部根据 :method 分发到具体方法)
 * state   : 处理器所需的上下文(随 handler 传入) */
typedef void (*qr_handler_fn)(qr_call_t* call,
                              const char* method,     /* 调用的完整方法名 "/包.服务/方法" */
                              void* handler_state);

qr_rc qr_server_register_service(qr_server_t* srv,
                                 const char* service,  /* "/包.服务" */
                                 qr_handler_fn handler,
                                 void* handler_state);

/* 服务端 accept 一个入站 call(事件驱动,见 §3)
 * 绑定层在 QR_EV_* 回调里处理;server 侧 call 由 kernel 创建并回调 */
```

### 4.5 buffer 与错误工具

```c
/* 释放 kernel 分配的接收 buffer */
void qr_buf_free(uint8_t* buf);

/* 取最近一次错误的字符串(kernel 持有,不释放) */
const char* qr_last_error(qr_conn_t* h);  /* h 可为 conn/call/srv 之一 */
```

---

## 5. 内存所有权约定

| 资源 | 分配方 | 释放方 | 接口 |
|---|---|---|---|
| `qr_cfg_t*` | kernel | 调用方 | `qr_cfg_free` |
| `qr_conn_t*` / `qr_server_t*` | kernel | 调用方 | `qr_conn_close` / `qr_server_close` |
| `qr_call_t*` | kernel | 调用方 | `qr_call_destroy` |
| `qr_call_send` 的 `data` | 调用方 | 调用方 | 内核在 send 返回前复制/消费完 |
| `qr_call_recv` 的 `*out` | kernel | 调用方 | `qr_buf_free` |
| `qr_last_error` / `qr_call_status` 返回的字符串/详情指针 | kernel | — | 不释放，下次调用可能失效 |

---

## 6. 线程安全约定

- `qr_init` / `qr_set_event_callback`：进程级，初始化期调用。
- 事件回调 `qr_event_cb` **可能在 kernel 的 tokio worker 线程触发**，绑定层必须线程安全地把它桥接到本地异步运行时：
  - Python：`asyncio.loop.call_soon_threadsafe` 唤醒主线程 Future。
  - Go：写入 channel（channel 本身线程安全）。
- 同一 `qr_call_t` 的并发调用（如同时 send/recv）由绑定层串行化；kernel 保证单 call 内部操作线程安全。

---

## 7. 错误处理约定

- 失败统一返回 `QR_ERR`（或 `QR_WOULD_BLOCK` / `QR_CLOSED` 的语义码）。
- 详细信息用 `qr_last_error` 取最近错误字符串（线程局部，调用失败后立即取）。
- 致命错误（连接断）：触发 `QR_EV_ERROR`，绑定层据此将 call 标记失败并清理。

---

## 8. 各语言绑定适配指引

| 语言 | 异步桥接 | 序列化 | 取消/超时 |
|---|---|---|---|
| **Python（PyO3）** | `QR_EV_*` → `call_soon_threadsafe` → asyncio Future | `protobuf` 库 | `asyncio.wait_for` + `qr_call_cancel` |
| **Go（cgo）** | `QR_EV_*` → channel → goroutine 阻塞读 | `google.golang.org/protobuf` | `context.Context` Done → `qr_call_cancel` |

---

## 9. 版本与兼容

- 1.0 前：ABI 可随实现演进，绑定层同步更新。
- 1.0：冻结 ABI。新增功能通过"新增函数/标志位"扩展，不改动既有签名与语义，保证向后兼容。
- 句柄布局对调用方不透明，内核可自由重构。

---

## 10. 设计决议（2026-07-23 评审通过）

| # | 议题 | 决议 |
|---|---|---|
| D1 | 是否提供同步阻塞变体 | **不提供**（绑定层自行封装同步语义） |
| D2 | metadata 传递结构 | **平行数组**（C 友好，见 §4.3 `qr_call_set_metadata`） |
| D3 | 连接级事件回调 | **与 call 级分离**（见 §3.1） |
| D4 | handler 注册粒度 | **按 service**（见 §4.4 `qr_server_register_service`） |

> 以上决议已并入正文（§3.1、§4.3、§4.4）。
