# qRPC 0.1 端到端打通

> **模板定位：** LLM 持续实施控制台  
> **功能 ID：** `qrpc-0.1`  
> **对应设计文档：** `docs/00-项目概述.md` … `07-路线图与里程碑.md`（已定稿，9 项设计决议已通过）  
> **对应需求：** `R-001 ~ R-018`  
> **功能分支：** `feature/qrpc-0.1`（待创建）  
> **优先级：** `P0`  
> **负责人：** `Owner (TBD)`  
> **最新代码校准日期：** `2026-07-23`（项目为绿地，尚无代码）

---

## 0. LLM-STATE（每轮实施前必须读取）

```yaml
schema_version: "1.0"
feature_id: "qrpc-0.1"
feature_name: "qRPC 0.1 端到端打通"
status: "todo"                  # todo | in_progress | blocked | partial | done | rolled_back | cancelled
current_task: "T-001"            # 当前应执行的任务；无则写 none
last_completed_task: "none"      # 最近完成任务；无则写 none
next_action: "执行 T-001：创建 Monorepo 骨架（core crate 分模块 + Cargo workspace + 顶层构建脚本）"
verification_status: "not_run"   # not_run | pass | fail | partial | skipped
research_status: "pass"          # 竞品/QUIC 栈/绑定机制调研已完成（见 01 §2.4）
call_chain_status: "partial"     # 绿地项目无现有调用链；目标调用链已设计（见 docs/02-HLD §5、03-LLD）
code_calibration_status: "not_run" # 无代码可校准；首次任务后开始
last_verification_command: "none"
last_updated: "2026-07-23T11:30:00+08:00"
resume_from:
  - "README.md#0-llm-state每轮实施前必须读取"
  - "01-实施计划.md#6-llm任务队列"
  - "02-实施状态与验证.md#3-执行日志"
allowed_scope:
  - "core/"
  - "rust/"
  - "protoc-gen-qrpc/"
  - "python/"
  - "go/"
  - "tests/"
  - "examples/"
  - "Cargo.toml"
  - "Makefile / justfile"
forbidden_scope:
  - "docs/（设计文档已定稿，仅可按已通过决议小幅更新，不得重构）"
stop_condition: "所有 required=true 的 T-### 均为 done，且 02 验证报告 VERDICT: PASS"
blocker_policy: "遇到 blocked 时记录 B-###、失败证据、已尝试方案和下一恢复动作；优先用最小修复而非扩大范围"
```

> **状态更新规则：** LLM 每完成一个任务，必须同步更新本块的 `current_task`、`last_completed_task`、`next_action`、`verification_status`、`code_calibration_status` 和 `last_updated`。

---

## 1. 一句话结论

> qRPC 0.1 处于 `todo`，尚未开始编码；架构与 9 项设计决议已定稿（见 `docs/`），Monorepo 结构已确定。下一步执行 **T-001：搭建 Monorepo 骨架**（`core/` crate 分模块 + Cargo workspace + 顶层构建脚本）。

---

## 2. 功能目标与非目标

### 2.1 目标

| R-ID | 目标 | 验收方式 | 状态 |
|---|---|---|---|
| R-001 | 内核帧协议（frame 编解码）正确 | `core/src/frame` 单测 + 黄金样本（C-001） | todo |
| R-002 | TAL 抽象 + quinn 后端可用 | 单测 + in-proc/quinn 双后端（C-002） | todo |
| R-003 | Call 状态机支持 4 种调用模式 | in-proc 集成测试（C-003） | todo |
| R-004 | server 方法路由（按 service） | dispatch 单测（C-004） | todo |
| R-005 | 连接管理（池化/keepalive/断连） | 集成测试（C-005） | todo |
| R-006 | 错误处理（gRPC 状态码 + STATUS） | 单测（C-006） | todo |
| R-007 | 超时 + 取消传播（含 server 清理） | 取消场景矩阵测试（C-007） | todo |
| R-008 | 背压（QUIC 流控传到用户代码） | 慢消费专项测试（C-008） | todo |
| R-009 | TLS 1.3 安全（rustls） | 集成测试（C-009） | todo |
| R-010 | C ABI 导出（call 级 + 连接级事件回调） | ABI 层测试（C-010） | todo |
| R-011 | Rust 绑定（直连内核，async API） | Rust 单测（C-011） | todo |
| R-012 | protoc-gen-qrpc 插件（三语言 stub） | 生成产物编译通过（C-012） | todo |
| R-013 | Python 绑定（PyO3 + asyncio） | Python 单测（C-013） | todo |
| R-014 | Go 绑定（cgo + channel/context） | Go 单测（C-014） | todo |
| R-015 | 跨语言互通（Rust/Python/Go 6 组） | 跨语言一致性套件（C-015） | todo |
| R-016 | 取消/背压跨语言一致 | 专项测试（C-016） | todo |
| R-017 | 性能基线（小包 RPS / 尾延迟） | criterion benchmark（C-017） | todo |
| R-018 | 跨语言示例 + 快速上手文档 | 示例可运行、文档可复现（C-018） | todo |

### 2.2 非目标

| 项 | 原因 | 后续处理 |
|---|---|---|
| MsQuic 后端 | 0.1 用 in-proc + quinn 验证 TAL；MsQuic 为 0.2 | NEXT-001（0.2） |
| RDMA 后端 | 第二阶段，需零拷贝贯通 | NEXT-002（2.0） |
| datagram 协同通道 | 0.3 差异化特性 | NEXT-003（0.3） |
| TCP 对外后端 | in-proc 仅测试用；TCP 降级为 2.0 | NEXT-004（2.0） |
| 拦截器 / 负载均衡 / 服务发现 / 重试 / 健康检查 / 反射 | 0.2~0.3 | NEXT-005 |
| Node / C++ / Java 等语言 | 0.3 起逐个 | NEXT-006 |
| 完整可观测性（metrics / 分布式 trace） | 0.3 | NEXT-007 |
| C ABI 冻结 | 1.0 才冻结，0.1 可演进 | NEXT-008（1.0） |

---

## 3. 目标调用链（绿地项目，无现有运行时）

### 3.1 调研与调用链证据摘要

| 项 | 状态 | 证据位置 | 结论摘要 |
|---|---|---|---|
| 官方文档调研 | pass | `01-实施计划.md §2.4` | QUIC RFC、gRPC 状态码、protobuf/protoc、quinn/rustls/PyO3/cgo 文档 |
| GitHub 开源实现调研 | pass | `01-实施计划.md §2.4` | IceRPC（仅C#）、quic-rpc（仅Rust）、grpc/grpc、Mercury/UCX |
| subagent/GitNexus 调用链分析 | partial | `01-实施计划.md §3` | 绿地无现有代码；分析的是目标链路（设计）而非现有调用链 |
| 当前代码实时校准 | skipped | `02 §8` | 项目未启动，无代码；首次任务后开始 |

### 3.2 目标调用链（Unary，Python client → Go server）

```text
用户调用 stub.GetUser(req)
  -> [绑定层] protobuf 序列化 -> bytes
  -> [C ABI] 句柄+字节+元数据
  -> [内核 call] 建 Call，开 bidir QUIC stream
  -> [内核 frame] 编码 METADATA + DATA
  -> [TAL/quinn] 写入 QUIC stream
  -> (QUIC over UDP / TLS 1.3)
  -> [对端内核] 解帧 -> dispatch -> C ABI -> 绑定层反序列化 -> handler
  -> 响应原路返回
```

关键约束：

1. 内核不感知序列化格式，只搬字节。
2. C ABI 是唯一跨语言契约，1.0 前可演进。
3. 流控复用 QUIC 原生，不自建。
4. 不得引入 HTTP/2 / HTTP/3 协议栈。

---

## 4. 当前实施状态总览

| T-ID | 任务 | Status | DependsOn | Evidence | Next |
|---|---|---|---|---|---|
| T-001 | Monorepo 骨架 + core crate 分模块 + Cargo workspace | todo | none | none | T-002 |
| T-002 | `frame` 帧编解码 + 单测 | todo | T-001 | none | T-005 |
| T-003 | TAL trait + in-proc 后端 + 单测 | todo | T-001 | none | T-004 |
| T-004 | quinn 后端 | todo | T-003 | none | T-007/T-009 |
| T-005 | `call` 状态机 + 4 模式 + 集成测试（in-proc） | todo | T-002, T-003 | none | T-006/T-010 |
| T-006 | `dispatch` + server 骨架（按 service） | todo | T-005 | none | T-010 |
| T-007 | `conn` 连接管理 | todo | T-004 | none | T-010 |
| T-008 | `error` + STATUS + gRPC 状态码 | todo | T-002 | none | T-010 |
| T-009 | `auth`（TLS 1.3 + token） | todo | T-004 | none | T-010 |
| T-010 | `abi` C ABI 导出 + 全局事件回调（call级+连接级） | todo | T-005,T-006,T-007,T-008 | none | T-013/T-014 |
| T-011 | Rust 绑定（直连 core） | todo | T-005, T-006 | none | T-012/T-015 |
| T-012 | `protoc-gen-qrpc` 插件（三语言 stub） | todo | T-011 | none | T-013/T-014 |
| T-013 | Python 绑定（PyO3 + asyncio） | todo | T-010, T-012 | none | T-015 |
| T-014 | Go 绑定（cgo + channel/context） | todo | T-010, T-012 | none | T-015 |
| T-015 | 跨语言一致性测试套件（6 组） | todo | T-011,T-013,T-014 | none | T-016/T-017 |
| T-016 | 取消/背压专项测试 | todo | T-015 | none | T-018 |
| T-017 | 性能基线 benchmark | todo | T-015 | none | T-018 |
| T-018 | 跨语言示例 + 快速上手文档 | todo | T-015 | none | none |

状态值只允许：`todo`、`in_progress`、`blocked`、`done`、`skipped`。

---

## 5. 交付物清单

| 类型 | 路径/对象 | 预期变更 | 状态 | 证据 |
|---|---|---|---|---|
| Code | `core/`（Rust 内核 crate） | 新增（frame/call/dispatch/conn/error/transport/abi/auth） | todo | none |
| Code | `rust/` | 新增 Rust 绑定 | todo | none |
| Code | `protoc-gen-qrpc/` | 新增代码生成器 | todo | none |
| Code | `python/` | 新增 PyO3 绑定 | todo | none |
| Code | `go/` | 新增 cgo 绑定 | todo | none |
| Test | `core/**/*_test.rs` | 新增内核单测/集成测试 | todo | none |
| Test | `tests/` | 新增跨语言一致性套件 | todo | none |
| Config | `Cargo.toml`（workspace）、`Makefile/justfile` | 新增 | todo | none |
| Doc | `examples/` + `docs/快速上手` | 新增 | todo | none |

---

## 6. 数据模型 / 配置 / 接口

> 详细规范见 `docs/`，本节仅指针。

### 6.1 协议数据模型
- 帧格式（8 字节头）：见 `docs/04-协议规范-RPC帧协议.md §3`。
- 帧类型（METADATA/DATA/STATUS）：见同上 §4–§5。
- 调用状态机：见 `docs/03-LLD §4.1`。

### 6.2 配置项

| 配置项 | 默认值 | 行为 | 兼容性 |
|---|---|---|---|
| 默认端口 | `7000`（决议 D1，可配置） | QUIC 监听端口 | — |
| 最大消息上限 | `4 MiB`（决议 D5，可配置） | 超过 → RESOURCE_EXHAUSTED | — |
| TLS | 强制 1.3（rustls） | 无明文模式 | — |
| keepalive | 可配置 interval/timeout | 心跳 + 断连检测 | — |

### 6.3 接口

| 接口 | 文档 | 兼容性 |
|---|---|---|
| C ABI（extern "C"） | `docs/05-接口规范-CABI契约.md` | 1.0 冻结前可演进 |
| TAL trait | `docs/03-LLD §2` | 内核内部，可演进 |
| 各语言用户 API | `docs/03-LLD §10` | 1.0 冻结 |

---

## 7. 验收标准

| AC-ID | 验收项 | 覆盖任务 | 验证命令/证据 | Status |
|---|---|---|---|---|
| AC-001 | 帧编解码正确（黄金样本 + 边界） | T-002 | `C-001` | todo |
| AC-002 | TAL 经 in-proc + quinn 双后端验证 | T-003,T-004 | `C-002` | todo |
| AC-003 | 4 种调用模式端到端（in-proc） | T-005 | `C-003` | todo |
| AC-004 | server 按 service 路由 + 未知方法 UNIMPLEMENTED | T-006 | `C-004` | todo |
| AC-005 | 连接池化/keepalive/断连检测 | T-007 | `C-005` | todo |
| AC-006 | 17 个 gRPC 状态码 + STATUS 正确 | T-008 | `C-006` | todo |
| AC-007 | 取消传播 + server handler 清理（无泄漏） | T-005,T-016 | `C-007` | todo |
| AC-008 | 背压传导（慢消费不 OOM） | T-005,T-016 | `C-008` | todo |
| AC-009 | TLS 1.3 强制加密 | T-009 | `C-009` | todo |
| AC-010 | C ABI call 级 + 连接级事件回调 | T-010 | `C-010` | todo |
| AC-011 | Rust 绑定 async API 可用 | T-011 | `C-011` | todo |
| AC-012 | protoc 插件生成三语言 stub 可编译 | T-012 | `C-012` | todo |
| AC-013 | Python asyncio 绑定可用 | T-013 | `C-013` | todo |
| AC-014 | Go cgo 绑定可用 | T-014 | `C-014` | todo |
| AC-015 | Rust/Python/Go 6 组互通一致 | T-015 | `C-015` | todo |
| AC-016 | 取消/背压跨语言一致 | T-016 | `C-016` | todo |
| AC-017 | 性能基线有数据 | T-017 | `C-017` | todo |
| AC-018 | 跨语言示例可运行、文档可复现 | T-018 | `C-018` | todo |

---

## 8. 验证入口

LLM 执行每个任务前必须先完成对应任务的 Pre-Implementation Check；完成任务后优先运行对应命令（命令清单见 `01-实施计划.md §6` 任务表与 `02-实施状态与验证.md §4`）。

```bash
# 内核（Rust）
cargo test -p qrpc-core                       # 全部内核测试
cargo test -p qrpc-core frame                 # 帧编解码
cargo test -p qrpc-core --test integration    # in-proc 集成
# 绑定
cargo build -p qrpc                           # Rust 绑定
maturin develop -m python/Cargo.toml          # Python wheel
( cd go && go build ./... )                   # Go 绑定
# 跨语言一致性
just test-interop                             # 6 组组合
# 性能
cargo bench -p qrpc-core                      # criterion
# 全量
just verify                                   # C-999
```

| C-ID | Command | 何时运行 | 预期 |
|---|---|---|---|
| C-001 | `cargo test -p qrpc-core frame` | T-002 后 | PASS |
| C-002 | `cargo test -p qrpc-core transport` | T-004 后 | PASS |
| C-003 | `cargo test -p qrpc-core --test integration` | T-005 后 | PASS |
| C-015 | `just test-interop` | T-015 后 | PASS（6 组） |
| C-999 | `just verify` | 全部任务完成后 | PASS |

---

## 9. 风险与回滚摘要

| RK-ID | 风险 | 影响 | 缓解措施 | 回滚方式 |
|---|---|---|---|---|
| RK-001 | PyO3↔asyncio 桥接（GIL/线程唤醒） | 高 | 早期最小桥接原型；pyo3-asyncio；并发压测 | 回退桥接实现 |
| RK-002 | C ABI 异步模型跨语言一致性（背压） | 高 | in-proc + 跨语言套件全覆盖；专项测试 | 修单语言绑定 |
| RK-003 | 取消传播正确性（泄漏） | 中 | TDD；取消矩阵；资源计数归零断言 | 修 call 取消路径 |
| RK-004 | cgo 开销（Go 性能） | 中 | send/recv 落 cgo 边界 + channel 调度；benchmark | 调度策略 |
| RK-005 | protoc 插件成熟度（schema 边界） | 中 | 早期覆盖常用 schema；渐进补边界 | 修生成器 |
| RK-006 | 范围蔓延 | 中-高 | 严守 Out 清单 + 里程碑 | 拒绝范围外 PR |
| RK-007 | QUIC server-to-server CPU 开销 | 中 | 基本盘为客户端场景；server 高吞吐靠 0.2 MsQuic | 接 MsQuic |

---

## 10. 文档索引

| 文档 | 用途 |
|---|---|
| [`00-使用说明.md`](00-使用说明.md) | LLM 执行协议、状态枚举、证据规范 |
| [`01-实施计划.md`](01-实施计划.md) | 详细任务队列、依赖、调研、调用链、文件范围 |
| [`02-实施状态与验证.md`](02-实施状态与验证.md) | 执行日志、命令结果、验收覆盖、最终 VERDICT |
| [`03-复盘与交接.md`](03-复盘与交接.md) | 决策、踩坑、回滚、维护与交接 |
| [`04-设计方案.md`](04-设计方案.md) | 设计方案（指针 → `docs/` 标准文档） |
| `docs/00~07` | 已定稿设计文档（SRS/HLD/LLD/协议/CABI/测试/路线图） |

---

## 11. 变更日志

| 时间 | 变更 | 作者/Agent | 证据 |
|---|---|---|---|
| 2026-07-23T11:30:00+08:00 | 创建 LLM 实施控制台（设计已定稿，9 项决议通过） | Claude (Fable) | `docs/` |
