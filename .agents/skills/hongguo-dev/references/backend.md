# 后端开发规范（crates/）

NSH 壳进程内直调 service；service / store / api / stream / media 等 crate 是纯业务层，
**不依赖任何 UI 框架**（有静态测试扫源码盯着）。

## 分层纪律

```
crates/hongguo-nsh（NSH 壳：页面 / 播放器 / 本机 HTTP 供数 / 装配）
└─ bootstrap.rs        # 构造 AppStateInner、起注册/通知/自动清理等常驻任务

crates/hongguo-service（业务编排，无任何 UI 框架）
├─ <域>_service/       # download/play/merge/login/series/settings/storage/transcode/capability/follow/history
├─ device.rs           # 设备档案（adopt / 注册换档），不是 *_service
├─ app_state.rs        # AppState = Arc<AppStateInner>（NSH bootstrap 直接构造）
├─ runtime.rs          # 共用任务运行时（后台任务任意线程可派发）
└─ events.rs           # ServiceEvent 业务事件总线（NSH 事件泵适配，契约见 references/ipc.md）
```

- 已迁出的 crate：`crates/hongguo-common`（领域模型 `model` / 统一错误 `AppError` / 纯工具
  `utils` / 跨端常量 `consts` / 媒体 URL 形态 `media_url`）、`crates/hongguo-signer`（请求
  签名，JS 1:1 禁改）、`crates/hongguo-video`（MP4 解析与 CENC 解密）、`crates/hongguo-api`
  （官方接口客户端：calendar / danmaku / detail / discover / emoticon / history / interact /
  login / new_drama / play_strategy / play_url / rank / recommend / register / reservation /
  search / stream_pick 等，另有 client / params；改端点前必读
  `docs/hongguo-api-endpoints.md`）、`crates/hongguo-store`（Turso 持久化：db 迁移 /
  `entity/` 八个模块 / bridge 同步门面）、`crates/hongguo-stream`（在线流契约：渐进流 /
  缓存 / Range / 进程级 `cache()` 单例）、`crates/hongguo-media`（纯 Rust 编解码：demux /
  hevc / mp4 写出层 / mux 封装 / remux / transcode / heif 等）。常量归属规则见 common 的
  `consts/mod.rs` 头注释。
- **媒体层三块**：纯 Rust 容器与封装在 `hongguo-media`；原生媒体编解码和能力探测在
  `hongguo-media-platform` 门面（Apple 走 `hongguo-media-videotoolbox`）及各平台实现 crate；
  平台请求契约在 `hongguo-media-types::encoding`。方向恒为「平台/能力探测 → 媒体核心」，
  禁止反向。
- **在线流的两侧分工（`hongguo-stream` 是唯一交点）**：填数据在
  `crates/hongguo-service/src/play_service/online/`（`hongguo_stream::cache()` 拿全局缓存，
  塞 `ProgressiveStream` / 明文），应答在 NSH 的本机 HTTP 供数（`crates/hongguo-nsh/src/media/`）。
  两边都只往下依赖 stream 契约——**别让应答层回头调 service**（历史上协议层直接调
  play_service 的缓存入口构成双向耦合，2026-10-11 修掉）。
- service 层硬纪律（有静态测试扫源码）：禁 `tokio::spawn` / `Handle::current` /
  `block_on`——同步 command 回调线程没有 Tokio 上下文，会 panic→abort；起后台任务一律走
  `hongguo_service::runtime::spawn` / `spawn_blocking`。
- **锁不跨 await**：`AppStateInner` 的字段全是 `RwLock`（parking_lot，guard 非 Send）。读
  状态走快照方法 `state.settings()` / `state.queue()` / `state.device()`（clone 出来用），
  绝不把 guard 带过 await 点。
- 外呼标准开头：`let env = state.api_env();`——一次调用链内环境（代理+设备+Cookie）不变，
  避免「签名用 A 设备、请求带 B Cookie」。
- service 层「归一化与校验一体」：校验的是归一化之后的结果，拆两个函数会留下顺序写反的
  窗口（settings_service 注释）。
- **持久化与传输分离**：`hongguo-store` 的 `entity/` 只被 bridge 转调，SQL 与行存储细节不出
  该 crate；跨层出入参一律是 `hongguo_common::model` 的 serde 模型。队列落盘这类
  「内存态 → 库」的编排留给上层（`service::download_service::persist_tasks`），store 只提供
  `Store` 原语——否则持久化层会反向依赖 `AppState`。

## store 层（`crates/hongguo-store`，Turso/SQLite）

**bridge.rs 同步门面**——专职 `hongguo-db` 线程串行跑 SQL，业务侧保持同步调用习惯。新增
访问器一行模板（参数提前 clone owned）：

```rust
pub fn series_by_id(&self, series_id: &str) -> AppResult<Option<Series>> {
    let id = series_id.to_string();
    self.exec(move |db| Box::pin(async move { entity::series_by_id(db, &id).await }))
}
```

**entity/**：八个模块（`device` / `follow` / `local_history` / `merge` / `playback` /
`series` / `settings` / `task`），全部 `pub async fn`（无 trait），只被 bridge 转调，业务
代码不直接碰。存储设计：复杂实体整行存 JSON 列，只把需要 WHERE/ORDER/JOIN 的字段提升为
列；playback 是最热写路径（播放期 5 秒节流上报）用全真实列 + UPSERT；多行写包单事务
（`db.with_tx`）。

**schema 迁移流程**（db.rs）：

1. `SCHEMA_VERSION += 1`；
2. `MIGRATIONS` 尾部追加一段 SQL（**只升一级**，注释写变更原因，并在注释里附**回滚 SQL**
   ——升级与回滚成对，不留只有升级的迁移）；
3. 约束变更走「建新表 → INSERT SELECT → DROP → RENAME」（SQLite 不能改约束）；
4. 多语句 DDL 必须 `execute_batch`（Turso 的 execute 只跑第一条）。

新实体全链路：db.rs 迁移 → `entity/` 对应模块读写 → bridge.rs 门面方法 → NSH bootstrap 启动装载。

## 错误处理

单一 `AppError`（thiserror）序列化 `{kind, message}`，18 个变体与 i18n 例外见
`references/ipc.md` 错误契约节。别合并：`StoreLocked`≠`StoreCorrupt`、`Busy`≠`InvalidArgs`、
`StreamMiss`≠`EmptyResponse`、`Internal`≠`Network`；`Cancelled` 供调度器；`Auth` /
`Rejected` / `StreamMiss` / `Unsupported` 为 transparent。队列表只存 i18n key 不存人话。

## 测试规范

- **组织**：内嵌 `#[cfg(test)] mod tests { use super::*; }` 与实现同文件；大文件测试拆兄弟
  文件 `#[path]` 挂回（`sample_table_tests.rs` 模式，能测私有函数）。
- **fixtures**：JSON 用 `serde_json::json!` 造抓包形状（注释标抓包日期）；MP4 用共享合成码流
  `crates/hongguo-media/tests/fixtures/`（`hongguo-video` 的 `test-support` feature 另有
  `mp4()/mp4_with_samples()` 造样本）——原则「只搭被测层真会读到的那几层」；磁盘用
  `temp_dir + process::id` 后清理；本地假 HTTP 用 TcpListener bind 127.0.0.1:0。
- **门控**：e2e 用 `HONGGUO_E2E_DIR` 没设就整体跳过（用 return 不用 expect，别让没数据的
  机器红一片）；数据目录重定向用 `ScopedDataDir`（RAII + 全局锁防互污，需
  `hongguo-store` 的 `test-support` feature）。
- **元测试钉决策**：本仓库惯用「扫源码断言约定」的测试（service 禁 tokio、事件形状稳定、
  重试次数 sane）。新立分层约定时照这个模式钉死。
- **跑法**：`cargo test --lib <模块过滤>`；平台门控组（MF/VT）按目标各跑，交叉验证
  `cargo clippy --target aarch64-apple-darwin`。

## 高频坑（每条都真踩过）

- 签名后不能动 url/body/query（连编码方式都失配）——业务代码永远不碰 signer，client.rs 内部接入。
- Turso `execute` 只跑第一条语句，多语句 DDL 用 `execute_batch`。
- 每次重试都要重新签名（时间戳过期）；重试耗尽报 `EmptyResponse` 不是 `Network`。
- 高频进度事件不节流会刷爆订阅方（<0.5% 且 <500ms 不发，`ProgressThrottle`）。
- `#[allow]` 被 lint 拒；用 `#[expect(reason)]`。
- 文件规模：拆分提醒线见 dev-habits『文件规模提醒』（约 300 行，往既有大文件堆方法同样算数）；
  **新增**源码文件超 800 行 CI 直接拒（`scripts/check-file-size.sh`）——别指望事后拆，写入时
  就按域分文件。
- release profile 禁 `panic = "abort"`。

## 动 media/platform 前必读

归档转码单入口 `pipeline::transcode` 使用原生平台 HEVC 编码器，不查找或调用外部 FFmpeg。
没有可用编码器时明确报错，不影响播放和同规格无损合并。跨平台验证保留原始时间轴；平台
编码的历史决策与 FFI 修复可查 git 历史。
