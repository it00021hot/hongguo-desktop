# REQ-HG-003 播放对齐交接

状态（2026-11-10，Asia/Shanghai）：**原桌面设备档案已取到《全球死考》第 13 集并在 macOS Tauri 出画。** 需求见 [issue #20](https://github.com/it00021hot/hongguo-desktop/issues/20)。工作分支为 `req-hg-003-official-apk-parity`，基线为 `3383873a020d385e2ddc9ba5ac6defc0b5fa267d`。

## 播放链路修复

Craft Anything 在官方 Cronet 的原生安全回调观察了实际签名前 URL、请求摘要和返回因子。以官方第 13 集相同输入离线对拍，Rust 的 `x-khronos`、`x-argus`、`x-ladon`、`x-gorgon`、`x-helios`、`x-medusa` 六项逐字节一致。Medusa 字段 13 覆盖 16 个选择器。真实桌面请求使用桌面自己的设备档案、运行时状态与随机量。

旧桌面签名缺少 MetaSec v5 会话。SDI 请求先前未通过校验。修正外层首字节与首个 XTEA 密文字节的异或关系，再用桌面当前档案补齐已证实的设备字段后，SDI 签发了本档案自己的 token。以同一档案发送第 13 集单集请求，HTTP 200，业务 `code=0`，解压体 24,605 字节。最终 token 按档案落在应用私有库，重启后读取的是状态 200 的记录。

配置 v5 后，显式选中的第 13 集使用单集路由。首集预取使用 `preload/v1`，后续只取当前集之后最多三集，走 `player/v1`。选中一部剧连播时暂停其他剧的媒体预取。匿名历史更新复用同一设备会话和 Cronet 六头，请求体只报当前一集。

本机验收使用测试目录 `target/parity/desktop-online-only`。它从同一桌面数据库复制档案和 token，下载目录独立且为空。本机 debug 壳从目标剧详情页打开，同一个进程先播第 1 集、切到第 2 集，再跳到第 13 集。三个 WebView 视频源都核对了 VID 和在线协议，并各取得三帧 1920×1080 的连续呈现回调。首包只含第 1 集，后续批量取第 2–4 集。第 13 集单集模型为 24,598 字节，匿名历史更新返回 62 字节且 `code=0`。

帧与脱敏请求记录在 `target/parity/desktop-wd/ep13-v5-sequence.json`。Craft 调查以 `fnd_32`、`fnd_33` 和 revision 88 的交接记录保留验收结果。

### 本地 SDK 配置

在应用数据目录的 `hongguo.db` 旁放置私有 `metasec.json`，字段为 `image`（APK 7.4.3.32 arm64 `libmetasec_ml.so` 的绝对路径）、`license`（该 APK 的应用级许可）、`environment_label`（同版本 MetaSec 的应用级环境标记）。本机验收目录已配置。应用会核验 ELF 的大小与 SM3，只读取其中的表及受限字节码。

调试命令也可通过 `HONGGUO_METASEC_IMAGE`、`HONGGUO_METASEC_LICENSE` 和 `HONGGUO_METASEC_ENV_LABEL` 指定相同输入。未配置本地 SDK 材料时仍走既有签名。配置材料与原始抓包保留在本机私有目录。发行包自动导入、远端 AB 所有分支与完整 Android 设备遥测尚未验收。

## 目标与证据位置

目标是在同一设备身份下，对齐官方的首次取流、后续预取、跳集与连续播放。换身份、扩大首包窗口、把后集塞进首包，都不能作为本需求的完成依据。

- 官方 APK：`/Users/milo/Downloads/红果免费短剧_7.4.3.32.apk`，包名 `com.phoenix.read`，版本 `7.4.3.32 / 74332`。
- APK SHA-256：`206b0b4c72e4516a24aabbcdc0aa6aacb19ccd8fba34f9d0cf9a03bfcb01b763`。
- Craft 调查项目：`/Users/milo/Documents/Craft Anything/7-4-3-32/investigation.cany`；本轮静态交接写入 revision 39，新增 `fnd_8`–`fnd_13`。
- 历史桌面实验在另一工作树 `/Users/milo/projects/hongguo-desktop/.qa-offline-launch-prep/`，不在当前分支的跟踪文件中；原始抓包、身份与票据不得提交。

## 已确定的边界

历史桌面匿名实验已经证伪下列“足以修复”的假设；它们没有官方运行时对照，不能外推成“官方后集也空”。这些实验发生在旧基线 `e623fb96`，本轮未重跑。

| 假设 | 已有结果 | 原工作树内报告 |
| --- | --- | --- |
| 首包后按连续 N=5 取后集，加 `dr_scene=player/preload` 即可非空 | 首包非空且媒体 Range 可读；后续新 vid 仍 HTTP 200、0 字节 | `.qa-offline-launch-prep/REPORT-SINGLE-IDENTITY-OFFICIAL-FOLLOWUP-202611101242.md` |
| 先上报进度、推进 index，再取连续后集即可非空 | 上报接口返回 SUCCESS，后续仍 0 字节；该设定下 `/player/v1` 对照亦空 | `.qa-offline-launch-prep/REPORT-SINGLE-IDENTITY-PROGRESS-FOLLOWUP-202611101254.md` |

上述实验仍未覆盖登录态、官方完整播放时序、N=3、约一分钟间隔等条件；进度实验的 `write_mode` 合并语义也未由回包证明。此前未完成项见同目录 `HANDOFF-HG003-SUSPENDED-202611101306.md`。

## 本轮官方静态规格

以下是从 APK 派生的规格，运行时 AB 与服务器响应仍需抓包确认。

| 结论 | Craft 证据 |
| --- | --- |
| `wb5.s0.f` 的缺省路径是 `multi_video_model/v1/`，不带 `dr_scene`；`video_player_api_dr_scene` AB 可选另一分支 | `ev_96d9639a7e2dffd22a632b2aab38d490b160aa195376f5cda3ceef24a9a24387` |
| `PreloadPrepareEnableABValue` 的 synthetic constructor 将 `preloadNum` 缺省设为 **3**；热点数为 0，低峰 WiFi/非 WiFi 数均为 3 | `ev_7d0b52149f93044a9e8f2ecade2f232414e1abb50212deaf1a70314035060a85`、`ev_261f2ff792b28847f26c04c3331f1898a02ec87e5baf41f93d9d1c4c6d7e8a6f` |
| 配置来自 `video_preload_prepare_v619`；`c95.g0.a` 依次选热点、suppress、低峰、常态；adapter 从 `D+1` 收集候选，常态缺省是后三个有效候选 | `ev_cd0826cb958c3911f72f1ffff51ea4891858731e0c5826f5ad9553e95d487e96`、`ev_97f1b1f8432e04df6bceb425b3a6c5e54749c847014433ffe752fd7d4249fed4`、`ev_c808860a60add14ce611e4eac54c53ac0f4ff0d850cf40cffda62d7ce4e653a9` |
| MetaSec 初始化读取 AppId、channel、安全 settings 与 fetched-did callback；注册回调同步 did/iid 并报告 `did-iid-update` | `ev_06bec6d8b37b77a44885cf1ad1d8c363272ebcf6aa6076a5411e13b3a378013b`、`ev_522a6971fc6d798eff257d4b78e572edba265e94a89448e3fa8a990d50599c52` |
| 首次安装报告 `first_time`，登录成功报告 `login`；公共参数按请求生成 `_rticket` 并读取当前 AB、网络与身份状态 | `ev_93cb96b91ba608270f11fbd4ef488b04ab7a7e88adc586b5ca618cc35bf58175`、`ev_ae6936d07808b2628ab92bcaf5b603571cb4e6ac4bf2b362343ea5aaf4c30f90`、`ev_4416f442a395ad791a31491bc3f07dacfe8d5b66328716887a9f2810c667b180` |
| MetaSec interceptor 请求分支产生 `x-bdms-payload`；响应侧读取 `x-bdms-dy`、`x-bdms-dy-sw`、`x-bdms-cj`、`x-bdms-ctrl`，处理控制状态 | `ev_4a147dd70b8a68117a6e331c18f5c7221af7cf118d467b2f53288dd88139026b` |

最后一项只证明拦截器存在这些机制，尚未证明播放请求命中、响应携带这些头或它们导致后集空包。`x-bdms-cj` 的报告映射是借贷/转账/红包，不应解读成播放专属机制。不得凭这些静态线索伪造安全状态或改写 signer。

## 同 vid 运行时对照（v5 接入前）

《全球死考》第一季 `series_id=7697459724635098174`，首集 `7697465535398677529`，ep13 `7697465038512065561`。官方取流域名为 `api5-normal-sinfonlinea.fqnovel.com`，设备与安装身份在这组请求间保持一致，请求无账号 token。

| 场景 | 官方 7.4.3.32 | 桌面同一测试档案 |
| --- | --- | --- |
| 首集 | `POST /novel/player/multi_video_model/preload/v1`，响应 10,111 压缩字节，`code=0`，包含首集 5 档 | 同路由、首集独立预取，HTTP 200 / 23,665 解压字节，5 档 |
| 后续 ep2–4 | `POST /novel/player/multi_video_model/player/v1`，响应 9,135 压缩字节，3 集模型 | 相同 3 个新 vid，HTTP 200 / 0 字节 |
| 跳到 ep13 | `POST /novel/player/video_model/v1/`，HTTP 200 / 4,396 压缩字节，解压 24,572 字节，`code=0`，5 档 ByteVC1，播放器出画 | 相同 vid、域名、业务 body，HTTP 200 / 0 字节 |

官方 ep13 的运行时截图见 `target/parity/official-signing-ep13-a.png`，抓包摘要在 `target/parity/official/summary-focused.json`。清理后的 `flows-focused.jsonl` 保留原始 `capture_row`。编号 5、18、42、44 对应首包、ep2–4、ep14–16、ep13，46 记录 ep13 取流后的 `read_history/update`。桌面摘要见 `target/parity/desktop-aligned-summary.jsonl`。该次探针只发送一次初始注册，沿用同一档案，读取注册回包下发的 `klink_egdi`，并使用本身份实际获得的匿名 Cookie。额外的同档案延迟请求仍为 0 字节，见 `desktop-delayed-summary.jsonl`。

## 冷启动与剩余差异

另一个隔离 Android 用户用于观察首次启动，保留了上述播放会话。官方 `capture_row` 125 的注册响应携带 `klink_egdi` 和 `ttreq`，137 的 `GET /passport/account/info/v2/` 下发 `odin_tt`，153 的 `GET /passport/general/login_guiding_strategy/` 下发两枚 CSRF Cookie 和 `odin_tt`。两条 Passport 请求走 `security.snssdk.com`。这些状态在取流之前已经存在，目标剧取流响应无 `Set-Cookie`。

桌面已对齐实抓注册 query，保存注册回包的 `klink_egdi`，并用实抓域名和业务字段构造单集请求。签名前只注入一次设备字段。在线播放遇到空体时原样返回，保留当前设备身份。观看进度上报采用实抓域名与两种索引口径，分别见第 28、29、46、122 行。实际探针表明这些取流改动加上匿名 Passport Cookie 仍不足以让后集返回模型。

官方 query 的 `_rticket` 位于公共参数第 4 项，没有 `ts` 和 `openudid`。桌面探针使用同一档案，按实抓顺序与编码重建 query，并用当前算法对最终报文重新签名，ep13 仍为 0 字节，见 `target/parity/desktop-ordered-query-summary.jsonl`。

用旧 `branch_of` 对官方原始 query、gzip 摘要和时间戳分类，18 条样本中有 5 条落在分支 1，其中包括官方成功的 ep13。这个分支号无法单独解释后集空体。

Craft 的运行时交接记录为 `fnd_14` 和 `fnd_15`，脱敏 HAR 派生证据 `ev_8959d36885578b3db1626ebde9d443ddab6d5af4b26d74ad615a861f0c4bea78`。

专用 MuMu 的原生 Cronet 起初选用 TTTrustStore，系统 CA 已受 Android 信任但取流 TLS 失败。采集在专用实例内选用平台证书验证器，并让 API 经 HTTP/1.1 代理。采集器未修改设备身份、请求体、签名或证书验证的返回值。当前保留专用实例的 ADB、Frida、root 和回环 CONNECT 调试配置供后续复用，观察器已退出。原始材料包含会话信息，保存在私有目录。用于评审的是脱敏摘要和实播截图。

## ep13 签名前后对照

Java 的 `addSecurityFactor` 可经 `CronetAppProviderManager` 和 `NetworkParams` 到达 MetaSec JNI 的 `0x03000001` 分支。Craft `fnd_16`、`fnd_17` 记录了这条静态路径。本次 ep13 成功请求走 Cronet 的原生回调。运行时在 `libsscronet.so+0x47db78` 观察输入并在返回点读取输出，回调指针指向 `libmetasec_ml.so+0x2a72b8`。两个已定位的 Java JNI 入口均未命中。所见 JNI 操作 `0x03000003` 属于 MetaSec 控制拦截器。Retrofit 阶段的 URL 仍含电量、会话计数等字段，比较签名前输入时以原生回调收到的 URL 为准。

| 对照项 | 官方实际回调 | 桌面同档案 |
| --- | --- | --- |
| 签名前 URL | 最终取流域名及单集路径，31 个有值的 query 参数，`_rticket` 在第 4 项，保留 `ac` 后的空段 | 当前 signer 排序不同，包含 `ts` 和 `openudid`，缺 `rom_version`；实验按官方顺序构造后仍为空 |
| 签名前请求头 | 19 个小写名值对，以 CRLF 分隔，含自己的匿名 Cookie、`x-reading-request`、`x-tt-trace-id`、`x-ss-stub`、Cronet UA 和三种压缩能力 | 回环取证显示现有桌面请求缺两个请求追踪头，匿名 Cookie 和 UA 形状不同，自动声明 gzip |
| 业务体与摘要 | 热态 319 字节 JSON 压到 217 字节，MD5 与 `x-ss-stub` 一致；另捕获带 `use_server_dns_scene` 的冷启变体 | 解压 JSON 与热态逐字节相同。现有 gzip 216 字节，摘要与自身报文一致。实验也发送了 217 字节一致的 gzip |
| X-Gorgon、X-Helios | 29 条可用相同混淆反解。Gorgon 帧为 `0405` 或 `0605`，SDK 字为 `0x040a0300`；Helios 渠道为 `646637794` | 旧 JS 向量使用 `4001`、旧 SDK 字及渠道。探针生成官方子头，保持桌面自有设备和会话 |
| X-Argus、X-Ladon、X-Medusa | Argus 为 LE32 时间戳，Ladon 是 4 字节且不等于 BE/LE 时间戳，Medusa 归一化版本为 5，本次热态解码 975 字节 | Argus 一致，Ladon 为 BE32 时间戳，Medusa v3 回环样本 743 字节 |
| 网络结果 | 热态复取 HTTP 200，`code=0`，模型字段非空。官方第 13 集已见连续画面 | 同档案仅换 Gorgon 或一起换 Gorgon、Helios，均为 HTTP 200、`Content-Length: 0`，没有 `Content-Encoding` |

桌面 A/B 沿用同一复用档案。第二轮用与官方热态逐字节相同的 gzip 和该桌面身份自行取得的 Passport Cookie。再补请求追踪头、按原设备指纹生成 Cronet 形态 UA，并声明官方压缩能力，仍是 HTTP 200、`Content-Length: 0`。回环也证明，如果服务器发来官方 Brotli 响应，当前桌面会收到非空压缩字节，因此解码差异解释不了这次空体。

Medusa v5 的随机数尾部仍可用旧掩码还原出稳定帧字段，内部字节布置和 v3 不同。已在 APK 原生库定位 v5 版本标记及四组候选置换表，沿虚拟机调用确认各表有独立使用点。旧算法未能还原 v5 的内部明文，暂未取得可验证的桌面 v5 签名。另一个隔离 Android 用户首次取流前收到了非空 `/sdi/get_token` 响应。桌面预热未处理该响应，现有证据不足以判定它是否参与 ep13 发货。

热态原生签名证据为 `ev_afd0828e244b0e221dd47cc92d157dd4d2c6010523695262e77a7c0596f32449`，线上回包为 `ev_98c132b68734ff475bd9595dbaa08cf65c1ff56d299ba718b19a1d5c7e612371`。运行时对照、回环报告及 A/B 结果在 `target/parity/craft-redacted/`。

调试 Tauri 使用只复用模式和隔离数据目录，库内活跃档案与输入文件完全一致，注册尝试为空。UI 已选中第 13 集，未挂载视频，未通过出画验收。本轮未将实验性签名写进产品路径，服务端拒绝条件仍待单变量实测。

## Medusa v5 升级

同一 MuMu 用户再次请求 ep13 时，原生回调实际选择双参数分支 `+0x2a9094 → +0x2a9d70 → +0x2a80a4`。选择器 `0x171` 命中注册处理器 `+0x2adb14`，其 VM 程序为 `+0xf72a0`，随后进入 `+0x113be0`、`+0x102900`。从内部返回到 Cronet 的六个因子逐字节相同。实际运行证据包括 `ev_117ca5519829b26630c7b27bcadd86e57581050340732a51d898f261c6693d87` 与 `ev_51d067dea08d66eb066d272bd70e3533649be536391862e5324fc11169b9bb3d`。

v5 的稀疏 AES 子步骤已独立重算。四套变体各用两组公开合成输入，31 字节数据加尾标记后两块 CBC 共 32 字节，八组完整输出均与 APK 一致。已对照请求的变体符合 `khronos & 3`，密文末字节写入 Medusa 偏移 24，前 31 字节分散嵌入其后 31 组。两个同身份请求恢复的 packed 标记均为 `0xca`。位布局和合成向量见 `ev_0455349d0798befd00769501626ed618fd28db7674d3bbda926174293fd37c8e`，可运行 `python3 target/parity/verify_medusa_v5.py` 复核。

APK 内置许可经 Base64、AES-128 ECB、RC4 与倒序掩码还原为 protobuf，SDK 从其中读取 `sign_key`。同一进程两次 ep13 的 AES key 和 IV 一致，分别为 `sign_key` 解码后前、后 16 字节的 MD5。使用许可的应用级材料，离线解开目标 ep13 的播放签名并逐字节重建 Medusa v5 封装。内部沿用旧 xmxor，v5 顶层为 20 字段，F14 是最终 query 的 SM3 前 6 字节。

F13 的实际入口为 `+0x2aac34`，输入 52 字节依次为最终 query 的 32 字节 SM3、16 字节 gzip MD5、4 字节小端 Khronos，第三参数也传完整 Khronos。ep13 单集样本的 `get_iv & 15` 为 10。APK AArch64 离线执行和纯静态原型都得到相同的前 16 字节，后 4 字节按 `LE32(sum_md5(前 16 字节) | 0x20)` 重算，完整 20 字节与官方 protobuf 字段 13 一致。静态原型为 `target/parity/f13-mode10-prototype/verify_ep13.py`，证据为 `ev_46d9f0f0feac9e7ae3bcb0ab8ba41a017df81ace3c969339e8720494002f06ac`。

同一 MuMu 档案复取目标 ep13 后，运行时对比确认 F13 第三参数等于完整 Khronos。Ladon 的 VM 调用设置线程局部 32 位值，随后读取该值，交换字节并编码为 Base64。两处取值与 Cronet 返回逐字节相同，证据为 `ev_9ca49c2f31f91c19e4af063bb92fd9182fb8bc61559bdea72a8185217f266cf9`。仍需还原 setter 的上游取值规则。

已解出的 Medusa 字段 16 与 SDK 的 `getToken()` 逐字节一致。首次启动抓包里，此字段在 `/sdi/get_token` 首次回包后出现，第二次交换后变更。请求 protobuf 字段 4 和响应字段 6 的外层均用许可里的 `common_key` 做 AES-128 CBC，内层使用随 nonce 变动轮数的 XTEA CBC，再解出带长度的 zlib 数据。两条保留响应由 APK 函数和独立实现解码，内部字段 1 均等于随后 SDK 用于签名的 token。两条请求的内部字段 1 是同一份 134 项设备数据，第二次请求的字段 2 等于第一次响应的 token。

这一阶段的 F13 原型先验证了目标请求选到的模式 10。随后补齐了全部 16 个选择器及桌面会话的签名生成和落库，结果见上文。

补做同档案会话对照，去重后确认 Passport Cookie 各一项且非空，ep13 仍为 HTTP 200 空体。再用原档案访问官方广告设置端点，取得其自己的 `odin_tt` 替换后，结果相同。现有预热已过滤五条带历史 `did` 且没有 SDK 请求体的 MetaSec 请求。

## Codex 使用 Craft Anything

当前 CLI 可直接使用，无需先修改 Codex 配置。每个 CLI 进程结束后 active target 不保留，Android 分析命令须显式带 `--path`。

```bash
CRAFT_CLI='/Applications/Craft Anything.app/Contents/MacOS/craft-anything-cli'
CRAFT_PROJECT='/Users/milo/Documents/Craft Anything/7-4-3-32/investigation.cany'
"$CRAFT_CLI" --project "$CRAFT_PROJECT" core-investigation-show
"$CRAFT_CLI" --project "$CRAFT_PROJECT" core-session-show
"$CRAFT_CLI" --project "$CRAFT_PROJECT" --deep android-method-show \
  --path '/Users/milo/Downloads/红果免费短剧_7.4.3.32.apk' \
  --class-name 'Lc95/g0;' --method-name a
```

继续调查前读已有 brief/plan/handoff，再更新 hypothesis、findings 和 handoff；结论引用 `ev_…`，工作记忆更新使用 revision。Craft 的 adb 工具面向已连接 serial；连接、代理与 HTTPS 采集仍用 adb/mitmdump，离线 HAR 可用 `web-recording-show`。该工具会删凭证并把 body 化成 outline，不代替字段值对拍；PCAP 工具不解密 HTTPS。

长期可在 `/Users/milo/.codex/config.toml` 添加最小 MCP 条目，再重启/重连客户端；本轮**未自动应用**：

```toml
[mcp_servers.craft-anything]
command = "/Applications/Craft Anything.app/Contents/MacOS/craft-anything-cli"
args = ["mcp"]
startup_timeout_sec = 30
```

正式接入前可运行 CLI `doctor` 和 `setup --client codex --dry-run` 审阅计划。自动 setup 另有 skill 更新及 `rebuild_start` 批准设置，静态调查无需该批准项。[Codex MCP 官方文档](https://learn.chatgpt.com/docs/extend/mcp?surface=cli)说明配置和重连方式。

## 本轮验证与验收

当前工具变更的针对性检查（在仓库根裸跑）：

```bash
python3 -m unittest discover -s scripts -p 'test_playback_capture_report.py' -v
python3 scripts/playback_capture_report.py --help
python3 scripts/playback_capture_report.py /absolute/private/official-flows.jsonl \
  --output /absolute/private/official-playback-report.json
git diff --check
```

现有 14 个 unittest 覆盖 gzip/base64、Brotli 解码与原始长度分离、重复 query 与身份关联、Cookie/签名脱敏、缺失/空体/截断区分、业务字段与公共头校验。报告脚本支持多份 JSONL 输入。

API 单测串行 167 通过、47 个按原标记跳过，播放 service 单测 30 通过、1 跳过，注册档案恢复回归通过。API 并行运行曾出现离线环境变量相关的代理测试失败，串行复跑通过。严格 Clippy 在未改动的 `hongguo-common/src/offline_qa/io.rs:102` 遇到基线 `collapsible_if`，未达到全量 lint 通过。

本次追踪的 Tauri 调试二进制构建通过，14 个抓包报告单测通过。修正增长媒体状态的类型谓词后，TypeScript 和 Vite 生产构建通过，增长媒体的 8 个测试通过。API 单测串行 168 通过、47 按原标记跳过。全仓库格式检查报告存量文件的排版差异。

v5 最终代码通过 Tauri debug 构建、457 个 Rust 单测以及单独运行的 APK F13 选择器对拍。改动的 TypeScript 文件通过 Prettier 与 ESLint。原严格 Clippy 另有 `device.rs` 旧注册门控函数的参数数量告警，两项存量规则单独降级后四个 Rust crate 的 Clippy 通过。

播放对齐验收必须同时满足：

- 官方与桌面用同一组目标集，分别记录其稳定设备身份；首次与后续 model 的请求、响应和安全/会话状态可对应。
- 同身份首播、连续切集、跳到未进首包的后集均有非空可解析模型、可取媒体与真实出画；不以缓存、Range 抽样或扩大首包替代出画。
- 按已证实的差异修正所属层并跑相关检查。本 issue 验收 Tauri 桌面播放，GPUI 不在本次发版范围。
