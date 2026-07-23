# qRPC 软件需求规格说明书（SRS）

| 文档属性 | 内容 |
|---|---|
| 文档名称 | qRPC 软件需求规格说明书（Software Requirements Specification） |
| 版本 | v0.1 (Draft) |
| 日期 | 2026-07-23 |
| 状态 | 草案 / 待评审 |
| 依据 | IEEE 830 / ISO/IEC/IEEE 29148 结构 |
| 关联文档 | 00-项目概述、02-HLD、03-LLD、04-协议规范、05-CABI契约 |

---

## 1. 引言

### 1.1 目的
本文档规定 qRPC（基于 QUIC 的跨语言 RPC 框架）0.1 版本的功能需求、非功能需求、外部接口需求与用例，作为概要设计（HLD）、详细设计（LLD）、协议规范、测试与验收的依据。

### 1.2 范围
覆盖 0.1 版本（端到端打通）的全部需求。明确标注范围外项（见 §8）。RDMA、MsQuic、datagram 协同、更多语言等延后需求仅作前瞻提及。

### 1.3 定义与缩略语
见 `00-项目概述.md §7 术语表`。

### 1.4 参考资料
见 `00-项目概述.md §8 参考资料`。

### 1.5 概述
§2 总体描述；§3 外部接口需求；§4 功能需求；§5 非功能需求；§6 用例；§7 数据/协议需求概述；§8 约束、假设与范围外项。

---

## 2. 总体描述

### 2.1 产品视角
qRPC 是一个**自包含的 RPC 框架**，由以下部分组成：
- **内核**（Rust）：RPC 协议层、传输抽象层（TAL）、连接管理、调用状态机、错误与流控、C ABI 导出。
- **语言绑定**：Rust 原生 crate、Python（PyO3）、Go（cgo）。
- **代码生成工具**：`protoc-gen-qrpc` 插件。
- **协议**：自研 RPC 帧协议（基于 QUIC stream）。

### 2.2 产品功能概述
- 基于 protobuf `.proto` 定义服务，自动生成多语言 RPC stub。
- 通过 QUIC 进行跨语言 RPC 调用，支持四种调用模式。
- 提供错误处理、超时、取消、背压等完整 RPC 语义。
- 支持传输层可插拔（首发 quinn，预留 MsQuic / RDMA / TCP）。

### 2.3 用户特征
- 后端/系统开发者（Rust / Go / Python），需要在微服务、云原生、边缘场景做跨语言 RPC。
- 关注性能（延迟、吞吐）、安全（内存安全、TLS）、跨语言一致性与开发体验。

### 2.4 运行环境
- **内核**：Linux（主要）、macOS、Windows；x86_64 / ARM64。
- **Rust 绑定**：支持上述平台的 Rust 稳定工具链。
- **Python 绑定**：CPython 3.8+，asyncio。
- **Go 绑定**：Go 1.21+，cgo。
- **网络**：UDP 可达（QUIC 基于 UDP）；IPv4 / IPv6。

### 2.5 设计与实现约束
- 内核必须用 Rust 实现（内存安全约束）。
- 跨语言边界必须通过稳定的 C ABI（契约不可轻易变更，见 1.0 冻结目标）。
- 不得引入 HTTP/2 / HTTP/3 协议栈（自研 RPC 协议）。
- 序列化主在宿主语言层完成（用各语言原生 protobuf）；内核不感知序列化格式。
- 流控必须复用 QUIC 原生流控，不自建传输级流控。
- 代码注释使用中文。

### 2.6 假设与依赖
- 依赖：quinn（QUIC）、rustls（TLS）、prost（Rust protobuf）、各语言原生 protobuf 库、protoc。
- 假设：UDP 在目标网络可达（如被防火墙拦截需降级到 TCP，属于 2.0）。
- 假设：各语言运行时支持所需的 FFI/异步机制（Python asyncio、Go channel + cgo）。

---

## 3. 外部接口需求

### 3.1 用户接口（API）
各语言提供地道 API：
- **Rust**：async/await API（tokio），client trait + server trait。
- **Python**：asyncio 协程 API，client 类 + server 抽象基类。
- **Go**：阻塞式 + channel + context API（符合 Go 习惯），client interface + server interface。

### 3.2 软件接口
- **输入**：`.proto` 文件（含 service 定义）。
- **protoc**：标准插件协议（CodeGeneratorRequest/Response）。
- **各语言 protobuf 运行时**：用于消息序列化。

### 3.3 通信接口
- **传输**：QUIC over UDP（RFC 9000），TLS 1.3（RFC 9001）。
- **应用层协议**：qRPC 帧协议（见 `04-协议规范-RPC帧协议.md`）。
- **默认端口**：**7000**（可配置）。

---

## 4. 功能需求

> 编号规则：`FR-<模块>-<序号>`。优先级：**M**=Must(0.1 必做)，**S**=Should，**C**=Could。

### 4.1 调用模型（CALL）

| 编号 | 需求 | 优先级 |
|---|---|---|
| FR-CALL-001 | 支持 Unary 调用（1 请求 → 1 响应） | M |
| FR-CALL-002 | 支持 Server Streaming（1 请求 → N 响应） | M |
| FR-CALL-003 | 支持 Client Streaming（N 请求 → 1 响应） | M |
| FR-CALL-004 | 支持 Bidirectional Streaming（N ↔ N） | M |
| FR-CALL-005 | 每个 RPC 映射到一条独立的 QUIC bidirectional stream | M |
| FR-CALL-006 | 单条 QUIC 连接上支持多 RPC 并发（多路复用） | M |

### 4.2 传输层（TRANSPORT）

| 编号 | 需求 | 优先级 |
|---|---|---|
| FR-TRANSPORT-001 | 提供传输抽象层（TAL），定义 `Transport` trait（reliable stream / unreliable msg） | M |
| FR-TRANSPORT-002 | 提供 quinn 后端实现 | M |
| FR-TRANSPORT-003 | 提供 in-process 后端（仅测试/降级用，不对外承诺） | M |
| FR-TRANSPORT-004 | TAL 设计允许未来接入 MsQuic / RDMA / TCP 后端而不改上层 | M(架构) / S(实现) |
| FR-TRANSPORT-005 | 连接管理：建立、复用、心跳（keepalive）、断连检测 | M |
| FR-TRANSPORT-006 | 支持 0-RTT（quinn 能力允许时） | S |

### 4.3 IDL 与代码生成（IDL）

| 编号 | 需求 | 优先级 |
|---|---|---|
| FR-IDL-001 | 基于 protobuf `.proto` 的 `service` 定义 RPC 接口 | M |
| FR-IDL-002 | 提供 `protoc-gen-qrpc` 插件，生成 Rust / Python / Go 的 client 与 server stub | M |
| FR-IDL-003 | 消息类型由 protoc 原生插件生成，qRPC 不重复实现 | M |
| FR-IDL-004 | 生成代码支持 unary + 三种 streaming 方法的签名 | M |
| FR-IDL-005 | 支持自定义元数据（metadata / headers）的传递 | M |

### 4.4 跨语言（LANG）

| 编号 | 需求 | 优先级 |
|---|---|---|
| FR-LANG-001 | 提供 Rust 绑定（原生 crate，绕过 C ABI 直连内核） | M |
| FR-LANG-002 | 提供 Python 绑定（PyO3 + asyncio） | M |
| FR-LANG-003 | 提供 Go 绑定（cgo + channel + context） | M |
| FR-LANG-004 | 任意两种语言间可互通（Rust↔Python↔Go） | M |
| FR-LANG-005 | 各语言 API 风格地道（符合该语言习惯） | M |

### 4.5 错误处理（ERR）

| 编号 | 需求 | 优先级 |
|---|---|---|
| FR-ERR-001 | 错误模型复用 gRPC 状态码（17 个标准码） | M |
| FR-ERR-002 | STATUS 帧携带 code + message + 可选 details | M |
| FR-ERR-003 | 错误在各语言映射为本地习惯（异常 / error / Result） | M |
| FR-ERR-004 | 区分应用错误（handler 返回）与系统/传输错误（内核检测） | M |

### 4.6 超时与取消（TIMEOUT）

| 编号 | 需求 | 优先级 |
|---|---|---|
| FR-TIMEOUT-001 | client 可指定 deadline（超时），通过 METADATA 帧传递 | M |
| FR-TIMEOUT-002 | client 与 server 双侧跟踪 deadline，超时返回 DEADLINE_EXCEEDED | M |
| FR-TIMEOUT-003 | 任一方可主动取消，复用 QUIC STREAM_RESET | M |
| FR-TIMEOUT-004 | 取消必须传播：client 断连/取消时，server 侧 handler 必须收到通知并清理资源 | M |

### 4.7 流控与背压（FLOW）

| 编号 | 需求 | 优先级 |
|---|---|---|
| FR-FLOW-001 | 复用 QUIC 原生 per-stream + per-connection 流控 | M |
| FR-FLOW-002 | 将 QUIC 流控产生的背压传导至用户代码（不无限堆积内存） | M |
| FR-FLOW-003 | C ABI 提供异步事件机制（可读/可写/完成/出错）供绑定层适配 | M |

### 4.8 安全（AUTH）

| 编号 | 需求 | 优先级 |
|---|---|---|
| FR-AUTH-001 | 强制 TLS 1.3 加密（基于 rustls） | M |
| FR-AUTH-002 | 支持可选的应用层 token 鉴权（通过 metadata） | S |
| FR-AUTH-003 | 支持自定义证书 / CA 配置 | M |

### 4.9 序列化（CODEC）

| 编号 | 需求 | 优先级 |
|---|---|---|
| FR-CODEC-001 | 默认序列化格式为 protobuf（各语言原生库完成） | M |
| FR-CODEC-002 | 宿主语言层提供 Codec 抽象，允许未来替换为 Cap'n Proto 等零拷贝格式 | M(架构) |
| FR-CODEC-003 | 内核不感知序列化格式，只收发字节 | M |

### 4.10 可观测性（OBS）

| 编号 | 需求 | 优先级 |
|---|---|---|
| FR-OBS-001 | 内核提供最小 tracing（连接/调用生命周期关键事件） | M |
| FR-OBS-002 | 完整 metrics / 分布式 trace 集成 | C(0.3) |

### 4.11 C ABI 契约（ABI）

| 编号 | 需求 | 优先级 |
|---|---|---|
| FR-ABI-001 | 提供稳定、最小、句柄式的 C ABI（extern "C"） | M |
| FR-ABI-002 | ABI 设计为全局单一事件回调 + readiness 查询模型 | M |
| FR-ABI-003 | ABI 在 1.0 冻结前可演进，冻结后保证向后兼容 | M(流程) |

---

## 5. 非功能需求

### 5.1 性能（PERF）
| 编号 | 需求 | 度量 |
|---|---|---|
| NFR-PERF-001 | 小包 Unary 高 RPS | 目标：对标 gRPC 同等或更优（benchmark 待定基线，0.2 出数） |
| NFR-PERF-002 | 低尾延迟 | P99 / P999 在小包场景优于或持平 gRPC-over-TCP |
| NFR-PERF-003 | 连接与 stream 创建开销低 | 支撑海量并发短 RPC |
| NFR-PERF-004 | 大流量吞吐（0.2+ MsQuic） | 接 MsQuic + GSO 后吞吐对标第一梯队 |

### 5.2 安全性（SEC）
| 编号 | 需求 |
|---|---|
| NFR-SEC-001 | 内核内存安全（Rust 所有权保障，避免 C/C++ 内存漏洞） |
| NFR-SEC-002 | 全链路 TLS 1.3 加密 |
| NFR-SEC-003 | 输入校验（畸形帧、超长消息、恶意 metadata）不导致崩溃 |

### 5.3 可靠性（REL）
| 编号 | 需求 |
|---|---|
| NFR-REL-001 | 取消/超时/断连场景下，server handler 资源正确清理，无泄漏 |
| NFR-REL-002 | 单个 RPC 异常不影响同一连接上的其他 RPC |
| NFR-REL-003 | 畸形输入不导致内核 panic（优雅返回错误） |

### 5.4 可维护性（MAINT）
| 编号 | 需求 |
|---|---|
| NFR-MAINT-001 | 分层分模块（transport / frame / call / dispatch / error / flow / abi），各组件单一职责 |
| NFR-MAINT-002 | 内核迭代不影响 C ABI 之上的绑定层（ABI 不变时） |
| NFR-MAINT-003 | 代码注释使用中文 |

### 5.5 可移植性（PORT）
| 编号 | 需求 |
|---|---|
| NFR-PORT-001 | 内核跨 Linux / macOS / Windows |
| NFR-PORT-002 | 支持 x86_64 / ARM64 |

### 5.6 互操作性（INTEROP）
| 编号 | 需求 |
|---|---|
| NFR-INTEROP-001 | 跨语言行为一致（同一 `.proto`，不同语言语义相同） |
| NFR-INTEROP-002 | 提供跨语言一致性测试套件 |

### 5.7 可用性 / 开发体验（USAB）
| 编号 | 需求 |
|---|---|
| NFR-USAB-001 | 各语言提供地道 API 与类型安全的 stub |
| NFR-USAB-002 | 提供快速上手文档与跨语言示例 |

---

## 6. 用例（关键场景）

| 用例 | 场景 | 涉及需求 |
|---|---|---|
| UC-001 | Rust client 调用 Python server 的 Unary 方法 | FR-CALL-001, FR-LANG-004 |
| UC-002 | Go client 订阅 Rust server 的 Server Streaming | FR-CALL-002, FR-LANG-004 |
| UC-003 | Python client 流式上传到 Go server（Client Streaming） | FR-CALL-003, FR-FLOW-002 |
| UC-004 | Rust 与 Go 之间双向流（Bidirectional） | FR-CALL-004, FR-FLOW-002 |
| UC-005 | client 设置 deadline，server 处理超时，client 收到 DEADLINE_EXCEEDED | FR-TIMEOUT-001/002 |
| UC-006 | client 主动取消正在运行的 server streaming，server handler 被通知并清理 | FR-TIMEOUT-003/004, NFR-REL-001 |
| UC-007 | server handler 返回业务错误（如 NOT_FOUND），client 收到对应状态码 | FR-ERR-001/003 |
| UC-008 | server 慢消费触发背压，client 发送被限流，内存不爆炸 | FR-FLOW-002, NFR-REL |
| UC-009 | 连接断开，该连接上所有进行中 RPC 收到 UNAVAILABLE | FR-ERR-004, NFR-REL-002 |

---

## 7. 数据与协议需求概述

- **协议规范**：见 `04-协议规范-RPC帧协议.md`（帧格式、帧类型、调用生命周期）。
- **C ABI 契约**：见 `05-接口规范-CABI契约.md`（函数集、异步事件模型）。
- **IDL 约定**：基于 protobuf service，扩展约定见 §4.3。

---

## 8. 约束、假设与范围外项

### 8.1 范围外项（0.1 不实现，延后阶段见路线图）
- MsQuic / RDMA / TCP 对外可用后端（0.1 仅 quinn + in-proc）。
- datagram 协同通道。
- 拦截器框架、客户端负载均衡、服务发现、重试。
- 健康检查、反射。
- Node / C++ / Java 等更多语言绑定。
- 完整可观测性（metrics dashboard / 分布式 trace）。

### 8.2 关键约束
- 内核 Rust；跨语言经 C ABI；不引入 HTTP 栈；序列化在宿主语言层；流控复用 QUIC。

### 8.3 关键假设
- UDP 在目标部署网络可达。
- 各语言运行时 FFI/异步能力满足（已在风险登记中跟踪）。

### 8.4 已知风险（详见 `07-路线图与里程碑.md §风险登记`）
- R1：PyO3 ↔ asyncio 桥接（GIL、线程唤醒）。
- R2：C ABI 异步模型跨语言一致性（背压正确性）。
- R3：取消传播正确性。
- R4：cgo 开销。
- R5：protoc 插件成熟度（schema 边界）。
- R6：范围蔓延。
- R7：QUIC 在 server-to-server 的 CPU 开销（靠 0.2 MsQuic + offload 缓解）。
