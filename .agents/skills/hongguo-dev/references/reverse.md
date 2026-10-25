# 逆向分析规范（官方 APK）

适用：考证官方客户端的协议形态、端点参数、数据模型、交互行为。逆向结论只是
「待验证假设」——进代码前必须按「对接官方接口禁止猜参数」「抓包三点一线」两条红线走
抓包「请求 + 响应 + UI 效果」核对。

## 铁律

1. **静态优先**：不运行目标、不抓包。动态抓包是另一条通道（`captures/addon.py` + mitmdump），两条通道的结论不许互相替代：静态拿到的是「形态假设」，抓包拿到的才是「可对齐口径」。
2. **大包禁整体反编译**：14 万类级别的 APK，先全量字符串表定位目标在哪个 dex，再对**单个 dex** 跑 jadx（秒级~分钟级）；整体反编译又慢又 OOM。
3. **路径在场 ≠ 接口可用**：静态提取的端点清单混有 H5 路由、废弃接口；对接口径一律以抓包台账为准。
4. **签名五件套（x-gorgon/x-argus/x-ladon/x-khronos）只在 native 层**（`libsscronet.so`）：Java/dex 字符串全量搜不到是正常现象，别在 Java 层找；本项目走 `signer/ticket.rs`，禁改写法（红线 4）。
5. 未实测发包的结论，写进文档时必须显式标注「未实测」。

## Android APK 流水线（URL/关键字 → 源码行号）

1. APK 副本去掉根目录 `DebugProbesKt.bin`（会被 jadx 记成重复类直接 fail-closed）。
2. 解出全部 dex，跑 `dexstrings.py` 生成 `strings.tsv`（每行 `classesNN.dex\t字符串`）；导出每 dex 类清单。
3. 已知 URL/关键字 → `grep -a`（二进制当文本）`strings.tsv` 定位 dex。
4. `find-str-refs.py <dex> "<短needle>"` 反查 const-string 引用类与方法——**先用短 needle**（`scan_qr` 不是完整 URL）圈候选类，完整 URL 留到最后定位行号。
5. jadx 只反编译命中的那一个 dex：`--no-res --no-debug-info -j 10`、`JAVA_OPTS=-Xmx14g`；**串行**跑（并行 JVM 会静默挂），日志与退出码落盘。
6. 源码里 grep 完整 URL 拿 `类:行号`，读上下文还原参数组装，再反查调用者。

三条操作铁律：**jadx 退出码 1 不一定是失败**（查 `sources/` 有无产物再判）；**长命令写成 `.sh` 脚本执行**（内联长命令会被传输层吞字）；**long needle 先换短 needle**。

## 工具与产物（本机已就位）

| 项 | 位置 |
| --- | --- |
| JDK 21 + jadx CLI 1.5.3 | `JAVA_HOME=C:/Program Files/Eclipse Adoptium/jdk-21.0.12.101-hotspot`；`$HOME/tools/jadx-cli/bin/jadx.bat` |
| 分析产物根目录 | `C:/Users/liu13/tools/rea/analysis/hongguo-apk/`（strings.tsv、find-str-refs.py、dex/、jadx-src*/、models-dump.json、so/） |
| jadx-headless-mcp | **弃用**——会话环境快照看不到新 env，且单类反编译 15 分钟起步易 OOM；其唯一价值（全类清单）已被 `dex-classes-*.txt` 静态产物替代 |

## 台账指针

- `docs/hongguo-apk-endpoints.md` —— 官方 APK 端点全量清单（静态提取，1302 条）
- `docs/hongguo-api-endpoints.md` —— 项目对接口径唯一台账（抓包锁定；含扫码登录等实测死路的结论记录）
