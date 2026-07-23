# qRPC 概要设计说明书（HLD）

| 文档属性 | 内容 |
|---|---|
| 文档名称 | qRPC 概要设计说明书（High-Level Design） |
| 版本 | v0.1 (Draft) |
| 日期 | 2026-07-23 |
| 状态 | 草案 / 待评审 |
| 关联文档 | 00-项目概述、01-SRS、03-LLD、04-协议规范、05-CABI契约 |

---

## 1. 引言

### 1.1 目的
本文档描述 qRPC 的系统架构、分层、组件划分、技术选型、数据流与部署视图，作为详细设计（LLD）与实现的依据。

### 1.2 范围
覆盖 0.1 版本的架构。传输抽象层（TAL）按可扩展设计，但 0.1 只实现 quinn + in-proc 后端。

### 1.3 设计原则
1. **分层解耦**：宿主语言层 / 绑定层 / C ABI / 内核协议层 / TAL / 后端，各层单一职责、接口稳定。
2. **序列化在宿主语言层**：内核不感知序列化格式，只收发字节——全语言零额外序列化成本。
3. **C ABI 是唯一跨语言契约**：稳定、最小、句柄式；内核可自由迭代。
4. **传输可插拔**：通过 TAL 抽象，QUIC 栈与传输族可替换，不波及上层。
5. **复用优先**：流控复用 QUIC 原生；消息类复用 protoc 原生插件；不重复造轮子。
6. **可测试性**：每层可独立测试（in-proc 后端、mock TAL、跨语言一致性套件）。

---

## 2. 系统架构

### 2.1 分层架构

```
┌────────────────────────────────────────────────────────────────┐
│ 宿主语言应用层  (Rust / Python / Go 用户代码)                     │
│   用户对象 ──[各语言原生 protobuf 序列化]──► 字节 payload         │
├────────────────────────────────────────────────────────────────┤
│ 代码生成层:protoc-gen-qrpc ─► 各语言 Stub(client / server)      │
├────────────────────────────────────────────────────────────────┤
│ 语言绑定层(薄):Rust 原生 crate │ Python(PyO3)│ Go(cgo)          │
├═══════════════════════════ C ABI(extern "C")════════════════════┤
│ 稳定契约:句柄(连接/流/调用)+ 字节 buffer + 元数据                │
├────────────────────────────────────────────────────────────────┤
│ 内核(Rust):                                                    │
│ ┌────────────────────────────────────────────────────────────┐ │
│ │ RPC 协议层:帧封装、方法路由、调用生命周期、4 种调用模式      │ │
│ ├────────────────────────────────────────────────────────────┤ │
│ │ 传输抽象层 TAL: trait Transport { reliable_stream();        │ │
│ │                          unreliable_msg(); ... }            │ │
│ ├──────────────┬──────────────┬──────────────┬────────────────┤ │
│ │ Quinn Backend│ MsQuic(0.2)  │ Rdma(2.0)    │ TCP/In-proc    │ │
│ │  (首发)      │              │              │ (in-proc:测试) │ │
│ └──────────────┴──────────────┴──────────────┴────────────────┘ │
└────────────────────────────────────────────────────────────────┘
```

### 2.2 关键边界
1. **序列化边界**：protobuf 序列化发生在绑定层（宿主语言），内核只搬字节。
2. **C ABI 边界**：唯一跨语言契约，句柄式。
3. **TAL 边界**：内核内部的可插拔点，换后端不波及上层。

---

## 3. 技术选型与依据

| 决策 | 选择 | 依据 |
|---|---|---|
| 内核语言 | Rust | 内存安全 + 现代开发效率；syscall 开销低，贴合小包高 RPS |
| QUIC 栈 | quinn（首发）| 纯 Rust、async 原生、生态好；可换 MsQuic（0.2，峰值吞吐）|
| 跨语言机制 | C ABI + 薄绑定 | gRPC C-core 验证；MsQuic 同为 C ABI 友好 |
| Rust 绑定 | 直连内核 | 内核选 Rust 的隐性福利，一等公民 |
| IDL/序列化 | protobuf + Codec 抽象 | 生态红利、protoc 全语言；为 RDMA 零拷贝预留 |
| 调用映射 | 每 RPC 一个 QUIC bidir stream | 与 gRPC 一致；享受 stream 独立性 |
| 帧协议 | 自研（无 HTTP）| 去 HTTP 复杂层，简洁 |
| 错误模型 | gRPC 状态码 | 迁移成本、跨语言一致 |
| 背压 | QUIC 原生流控 + 上传 | 天然防 OOM |

> 依据细节见 `00-项目概述.md §6` 与竞品调研结论。

---

## 4. 组件设计

### 4.1 内核组件

| 组件 | 职责 | 主要接口（概要） | 依赖 |
|---|---|---|---|
| `transport`（TAL）| 传输抽象 + 后端 | `Transport`/`Connection`/`Stream` trait | quinn / MsQuic |
| `conn` | 连接管理：池化、心跳、重连 | `ConnectionPool::dial` | transport |
| `frame` | 帧编解码（8B 头 + payload）| `encode(Frame)` / `decode(buf)` | 无 |
| `call` | 调用状态机：4 模式生命周期 | `Call::new / send_msg / recv_msg / finish` | frame, transport |
| `dispatch` | server 方法路由 | `register(handler)` / `dispatch(method)` | call |
| `error` | 状态码 + STATUS | `ErrorCode`, `to_status()` | frame |
| `flow` | 背压（对接 QUIC 流控）| backpressure 信号上抛 | transport |
| `abi` | C ABI 导出 | `extern "C"` 函数集 | call, conn, error |
| `auth` | TLS + 可选 token | `TlsConfig`, `Authenticator` | rustls |
| `obs` | 最小 tracing | 关键事件日志 | — |

### 4.2 绑定层组件（各语言一致的四项职责）
1. 句柄 ↔ 语言对象映射
2. 异步桥接（全局事件回调 → 本地异步模型）
3. 序列化胶水（对象 ↔ protobuf 字节 ↔ C ABI data/len）
4. 错误映射（STATUS/ERROR → 本地异常）

### 4.3 代码生成
`protoc-gen-qrpc`：输入 `.proto` service，输出各语言 client/server stub。消息类用 protoc 原生插件。

---

## 5. 数据流

### 5.1 Unary 调用端到端流（Python client → Go server）

```
Python Client:
  用户代码 stub.GetUser(req)
    → ① protobuf 序列化 → bytes（宿主语言层）
    → PyO3 绑定: qrpc_call(conn, method, bytes, meta)
    ╞═══ C ABI(句柄+字节+元数据)═══
内核:
    → call: 建 Call，开 bidir QUIC stream
    → ② frame: 编码 METADATA + DATA（内核只组帧）
    → TAL/Quinn: 写入 stream
    → ③ QUIC over UDP / TLS 1.3
    ═══════ 网络 ═══════
Go Server:
    → Quinn: 从 stream 读字节
    → ④ frame: 解码 METADATA(method) + DATA(payload)
    → dispatch: method → handler
    → ⑤ call: payload 经 C ABI 交绑定层
    ╞═══ C ABI ═══
    → cgo 绑定: bytes → protobuf 反序列化 → Go 对象
    → 用户 handler 执行 → 返回 response
响应路径对称。
```

### 5.2 三个关键事实
1. 序列化只在绑定层（①、Go 侧）。
2. C ABI 传句柄 + 字节，不传对象。
3. 内核三件套（call / frame / transport）解耦。

---

## 6. 部署视图与代码库组织

### 6.1 运行时部署
- **库形态**：内核编译为静态/动态库 + Rust crate；各语言绑定打包为该语言包（PyPI / Go module / crates.io）。
- **运行**：用户应用嵌入绑定库；服务端监听 UDP（QUIC），客户端发起 QUIC 连接。
- **无中心组件**：点对点 RPC，不依赖独立 broker/registry（服务发现为 0.3+ 可选）。

### 6.2 代码库组织：Monorepo（决议）

qRPC 采用**单仓库（Monorepo）**组织。核心依据：内核与各绑定**共享 C ABI 契约、强耦合**，拆仓库会导致版本漂移与跨语言不一致。

**Monorepo 结构：**

```
qRPC/
├── core/                  # Rust 内核 crate(qrpc-core:C ABI 导出)
│   └── src/{transport,frame,call,dispatch,error,flow,conn,abi}/
├── rust/                  # Rust 绑定(直连 core,一等公民 async API)
├── protoc-gen-qrpc/       # 代码生成器(Rust 工具)
├── python/                # Python 绑定(PyO3 + maturin,链接 core)
├── go/                    # Go 绑定(cgo,链接 core 的 C ABI)
├── tests/                 # ⭐ 跨语言一致性测试套件(6 组组合)
├── examples/              # 各语言示例
├── docs/                  # 设计文档(本目录)
├── Cargo.toml             # Rust workspace(core + rust + protoc 插件)
└── Makefile / justfile    # 顶层构建编排
```

**为什么 Monorepo（而非多仓库）：**

| 论据 | 说明 |
|---|---|
| C ABI 契约联动 | 内核改 ABI → 所有绑定同步改，单次原子提交；多仓库会版本漂移 |
| 跨语言一致性测试 | 6 组语言组合互通测试需同 repo 同步构建 |
| 共享内核构建产物 | PyO3 / cgo 共同链接内核编译产物，build 一次各绑定共享 |
| protoc 插件联动 | 生成器 ↔ 生成代码 ↔ 绑定，紧密耦合 |
| 先例 | grpc/grpc 主仓库即 Monorepo（C++ 核心 + 基于 C-core 的多语言绑定） |

**Monorepo 的工程代价：** 需顶层构建编排（`just` / `Makefile` + CI 矩阵）协调 `cargo build`(core) → maturin(python wheel) → cgo(go) → 跨语言测试。代价远小于跨仓库的一致性风险。

**何时考虑拆分：** 仅当某语言是纯重写（不基于内核，如 grpc-java）或 1.0 后治理需要——0.1 不满足，故采用 Monorepo。

---

## 7. 接口设计指向

| 接口 | 文档 |
|---|---|
| RPC 帧协议（wire）| `04-协议规范-RPC帧协议.md` |
| C ABI 函数契约 | `05-接口规范-CABI契约.md` |
| 模块内部接口（trait 签名等）| `03-LLD-详细设计说明书.md` |

---

## 8. 关键设计决策与权衡

| 决策 | 权衡 | 选择理由 |
|---|---|---|
| MsQuic 进 0.1 还是 0.2 | 0.1 两后端可早验 TAL，但拖慢首发 | 放 0.2；0.1 用 in-proc 验证 TAL 抽象完整性 |
| C ABI 异步模型 | 回调式 vs 轮询式 | 全局单一事件回调 + readiness，兼顾效率与跨语言 |
| Rust 绑定走 C ABI 否 | 统一 vs 直连 | 直连内核，Rust 一等公民 |
| Codec 抽象位置 | 内核 vs 宿主语言 | 宿主语言层；内核不感知序列化 |

---

## 9. 横切关注点

| 关注点 | 0.1 | 后续 |
|---|---|---|
| 认证 | TLS 1.3 + 可选 token | 拦截器框架（0.2） |
| 可观测性 | 最小 tracing | metrics / 分布式 trace（0.3）|
| 负载均衡 | 无 | client lb + 名字解析（0.3）|
| 拦截器 | 无 | 0.2 |

---

## 10. 非功能设计映射

| NFR | 设计支撑 |
|---|---|
| NFR-PERF（小包高 RPS）| quinn 低 syscall + 每 RPC 独立 stream + 无 HTTP 开销 |
| NFR-SEC（内存安全）| Rust 内核 |
| NFR-REL（资源清理）| 取消传播 + per-call 状态机 |
| NFR-MAINT（分层）| 组件单一职责 + 稳定 ABI |
| NFR-INTEROP（跨语言一致）| 统一帧协议 + 一致性测试套 |
