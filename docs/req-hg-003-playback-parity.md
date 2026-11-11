# REQ-HG-003 播放对齐交接

首轮状态（2026-11-10，Asia/Shanghai）：**原桌面设备档案已取到《全球死考》第 13 集并在 macOS Tauri 出画。** 需求见 [issue #20](https://github.com/it00021hot/hongguo-desktop/issues/20)。首轮分支为 `req-hg-003-official-apk-parity`，基线为 `3383873a020d385e2ddc9ba5ac6defc0b5fa267d`。

## `dev` 播放生命周期复查

原 MuMu 用户 10 连续实测《全球死考》第一季第 1、13、14 集。首集进入时发历史和一集进度，切集及回访只更新选中集历史。返回第 13 集恢复 40,522 毫秒，累计 72,097 毫秒，复用同一模型。退出时更新当前集，并批传本轮观看的 3 集，全部 HTTP 200、`code=0`。短暂回访后退出无新增上报。第 13 集初次切入位置为 0，累计为 15,559 毫秒。桌面旧 5 秒首报、60 秒周期来自参考桌面端 v1.1.6。

从「系列剧」切至第二季首集再切回第一季，官方两次都依序发旧季历史、新季历史、两季进度批包，批包中新季在前。第二季起点为 0，第一季恢复第 14 集的位置 15,514 毫秒。两季累计分别记账，六条响应均为 HTTP 200、`code=0`。

桌面现保留完整播放模型，切档按当次能力和目标重选，同一档的流缓存可续填。两个壳的 v5 起播都只预热首集。Tauri 切集收尾读取最近一次位置采样，同集同位置的上报合并。兼容成片完成时同步最终文件长度，避免编码器直接完成而未回调首段时，已完成文件的 Range 被判空。

原桌面档案复测第 1、2、13 集均有在线真实帧。第 13 集由 1080p 切到 720p 再回自动，画面均实际呈现，单集模型请求数未增加。第 14 集切到 720p 后，当前兼容链仍从自动档选输入；新生成的 720p 文件 Range 回 206，桌面实际出画。脱敏帧数据在 `target/playback-behavior/`。

Tauri 匿名会话已改为进集更新、按剧累计实际播放时长、切剧收尾和离开播放器批传。「系列剧」入口选择该季上次播放的分集与位置。原桌面档案实测第 1 集发两条、切第 13 集只发历史、离开后更新第 13 集和上传两集进度。第 13 集保持在线 1080p 实际出画。采样时序与脱敏请求摘要在 `target/playback-behavior/official-sequence/`。退出的精确门槛、进程重启和登录态尚待实测。

双向切季验收经过第一季第 13 集、第二季第 1 集、第一季第 13 集，三段均取得在线 1920×1080 真实帧。切走前第 13 集在 52.38 秒，切回首帧在 52.53 秒。两次切季均依序成功更新旧季和新季历史并上传两集进度。桌面历史页显示第一季第 13 集 `00:52` 以及第二季记录。官方脱敏记录见 Craft `fnd_36`、`fnd_37`。

### 首页预取与媒体磁盘缓存

官方 `j45.e0.accept` 在分集档案返回后把候选交给 `j45.d1.a`。该方法对 `OUT_FEED` 写 `dr_scene=preload`，其余写 `player`。本次档案抓包有四部一批，既有接口记录有六部一批。Tauri 复用原首页候选窗口，将缺失档案按最多六部一批补齐，再按入口集的 `content_type` 合批取 `preload/v1`。同剧 v5 的模型窗口仍是后续最多三集。

`k45.g.d` 根据高峰、压制、低峰或常态选择字节数，再传给 `PreloaderVideoModelItem`。原 MuMu 实读常态为三集、`5242880` 字节，高峰为零集、`1048576` 字节，低峰 WiFi 和非 WiFi 均为三集、`1048576` 字节。该档案命中高峰时，候选数实际为零。官方 AB 回包的 `peak_time_v629` 给出 19:15–23:00，随机延后最多 45 分钟，压制区间从 23:00 起算 85 分钟再随机延后最多 20 分钟。普通设置回包的 `video_preload_prepare_v619.preload_suppress` 给出 `3x5242880`、`2x5242880`、`1x5242880` 三档。

桌面已移除预取集数设置。两个壳的首页和同剧入口共用动态候选数和字节预算，按 AB、普通设置、内置默认值取配置，使用本地时钟按高峰、压制、低峰、常态顺序选档。桌面按设备档案在内存缓存配置五分钟，普通设置增量保留先前的相关字段。候选窗口仍最多三集，高峰零候选时跳过后台取流。缓存已覆盖的区间直接复用，媒体元数据计入后台预算，兼容转码随实际选中分集触发。实际点播和档位沿用现有路径。官方 `300 ms` 是单剧播放器准备 Runnable 的首帧判定参数，首页刷剧的实际触发时机和入口集选择尚未完成单剧样本对照。

原桌面档案实测同时收到 AB 配置和普通设置的三档压制预算。高峰时首页和同剧入口均选零候选，无后台批量模型请求，第 13 集在线 1920×1080 实际出画。常态、压制、低峰和跨午夜选档由确定性测试覆盖。脱敏官方配置回包摘要见 Craft `fnd_43`。

MuMu 的 `cache/short` 和 `cache/dataloader` 有 `.mdl` 与 `.mdlnodeconf`。第 13 集暂停、播放和拖动时，同一个 3,666,355 字节文件的读取位置从 786,432 跳到 3,204,811。`AVMDLDataLoader` 运行时容量配置为 `209715200` 字节，`mMaxCacheAge` 原始值为 `1209600`。其期限单位与实际淘汰触发条件未证实。

桌面 `online-cache/` 保存原始加密媒体区间，键按设备、分集、档位和模型 `file_hash` 隔离，容量上限 200 MiB。URL 轮换不会让同一文件失去缓存命中。设置页单独显示在线磁盘占用，并与其他缓存一起清理。超容量时桌面按写入时间清除最旧区间，官方过期与逐出策略待对照。

只用《全球死考》原档案复测。冷取流第 13 集实际出画，清理归零后再次取数出画。重启后从磁盘复用第 13 集的 6,378,037 字节，取得 1920×1080 真帧。新版首页预取入口提交同剧两季，`preload/v1` HTTP 200、响应 40,552 字节。第二季从磁盘复用 2,352,555 字节，再向 CDN 补缺段，到 5,242,880 字节停止。脱敏回执在 `target/playback-behavior/official-sequence/`。

## Tauri ByteVC2 直播放

Tauri 播放选中 ByteVC2 时，vvdec 按需解出 I420 帧，原 AAC 音轨提供播放时钟。拖动进度时，从目标前的关键帧重新解码。Windows 通过 WebView2 共享缓冲交付画面，WKWebView 通过同一流协议接收二进制帧批次。在线复用原流的明文 Range，本地沿用视频路径校验并逐段读文件。

2026-11-11 用 Windows Tauri 开发构建验证《百岁老人在禁区修仙》第 3 集 720P。该集 720P 无缓存时点选档位到首帧为 424 毫秒，首帧在媒体下载完毕前解出。跳到 33 秒后 509 毫秒出画，首帧 PTS 为 33.000 秒。1× 连播约 30 秒零丢帧、零重新缓冲。显示帧与 AAC 时钟偏差 P95 为 37 毫秒，最大 44 毫秒。暂停后跳转与切到下一集通过。数据记录位于本地 `target/agent-tauri-bytevc2-direct/online-1x-results.json`。

直播放画布已接入锐化和 Anime4K。ByteVC2 解码、音轨或传输失败时进入现有播放错误流程，重试仍走 ByteVC2 直播放。

在 Windows Tauri 打开直播放样本并出画后，可用 `node scripts/bytevc2-playback-audit.mjs` 采集连续快速跳转结果、1× 到 3× 的显示帧数、丢帧数、重新缓冲次数和 PTS 偏差。脚本复用 [WebView2 CDP 调试端口](./webview-cdp-testing.md#启动唯一前提)，可传每档采样的秒数，默认 8 秒。安装包验收时对目标安装包的进程开启同一调试端口，并记录应用构建和 WebView2 运行时版本。

Windows 实机还需验收本地文件、画质增强、安装包、不同 WebView2 版本和其他音轨组合。弱网中断恢复、长时间连播及最终版本的 2×、3× 同样待测。早期 2× 曾出现丢帧。当前直接播放仍使用原 AAC 音轨，PTS 偏差是软件时钟指标。测实际输出时用同一外部录制设备记录屏幕闪帧和扬声器声脉冲，对照源里两者同步的测试片段。

macOS Tauri 用《全球死考》第 14 集 720P 验证开发地址与内嵌前端都能读取原 AAC 音轨并绘制 1280×720 I420 视频帧。内嵌前端暂停拖动到 21 秒后，音频位置和画布 PTS 均为 21.000 秒。隔离目录没有新增兼容转码文件。测试窗口处于隐藏状态，使用测试定时器驱动画布，正常窗口的连续播放和倍速指标待验收。

首页和详情页的在线起播遵循 `preferOnline`，同集下载记录不会遮蔽在线档位。显式文件和本地播放请求仍按本地来源解析。macOS 可见窗口实测《全球死考》第 1 集从 1080P 切到 720P，原播放点保留，ByteVC2 画布为 1280×720，切换后前 62 帧零丢帧。

### GPUI 直播放迁移计划

1. 将索引复用、Range 供数和有界解码队列归入现有媒体层。Tauri 保留协议与帧交付，GPUI 原生播放器消费同一份解码会话。换档、跳转和切集统一取消旧解码会话。
2. macOS 让 AVPlayer 通过现有 ResourceLoader 播原 AAC，把 I420 帧送进当前舞台的原生视频层。Windows 让 MediaEngine 通过现有 IMFByteStream 播原 AAC，把解码帧上传到现有 D3D 视频位。两端都按音频时钟选择 PTS，复用 GPUI 的几何、暂停和缓冲状态。
3. GPUI 在线和本地播放遇到 ByteVC2 时统一进入直播放，移除在线起播中的 H.264 转码分支。分别在 macOS 和 Windows 验收首帧、暂停拖动、连续跳转、换档、切集、1× 到 3× 和音画同步，并检查转码缓存目录。

## 播放链路修复

Craft Anything 在官方 Cronet 的原生安全回调观察了实际签名前 URL、请求摘要和返回因子。以官方第 13 集相同输入离线对拍，Rust 的 `x-khronos`、`x-argus`、`x-ladon`、`x-gorgon`、`x-helios`、`x-medusa` 六项逐字节一致。Medusa 字段 13 覆盖 16 个选择器。真实桌面请求使用桌面自己的设备档案、运行时状态与随机量。

旧桌面签名缺少 MetaSec v5 会话。SDI 请求先前未通过校验。修正外层首字节与首个 XTEA 密文字节的异或关系，再用桌面当前档案补齐已证实的设备字段后，SDI 签发了本档案自己的 token。以同一档案发送第 13 集单集请求，HTTP 200，业务 `code=0`，解压体 24,605 字节。最终 token 按档案落在应用私有库，重启后读取的是状态 200 的记录。

在线播放统一使用 v5。显式选中的第 13 集使用单集路由。首集预取使用 `preload/v1`，后续只取当前集之后最多三集，走 `player/v1`。选中一部剧连播时暂停其他剧的媒体预取。匿名历史更新复用同一设备会话和 Cronet 六头，请求体只报当前一集。

本机验收使用测试目录 `target/parity/desktop-online-only`。它从同一桌面数据库复制档案和 token，下载目录独立且为空。本机 debug 壳从目标剧详情页打开，同一个进程先播第 1 集、切到第 2 集，再跳到第 13 集。三个 WebView 视频源都核对了 VID 和在线协议，并各取得三帧 1920×1080 的连续呈现回调。首包只含第 1 集，后续批量取第 2–4 集。第 13 集单集模型为 24,598 字节，匿名历史更新返回 62 字节且 `code=0`。

帧与脱敏请求记录在 `target/parity/desktop-wd/ep13-v5-sequence.json`。Craft 调查以 `fnd_32`、`fnd_33` 和 revision 88 的交接记录保留验收结果。

### MetaSec v5 自动初始化

首次在线取流时，两个壳自动从 [红果官网下载落地页](https://applink.novelquickapp.com/FK6cu) 指向的固定 7.4.3.32 APK 获取所需的两个 HTTPS Range，共 5,413,035 字节。响应的区间、压缩长度和 SHA-256 均需匹配，解压长度有上限，ELF 还会核验 SM3。桌面读取其中的表及受限字节码。

校验通过的两个压缩片段保存在应用数据目录的 `metasec-v5-74332.cache`，与设备会话分开存放。后续启动重新校验并复用，缓存损坏会自动重取。安装后打开即可在线初始化，无需手动创建配置文件、下载 APK 或设置环境变量。发行包与源码均无需附带官方签名材料。

首次创建桌面设备库时写入初始注册记录，再由启动流程自动注册。已有窗口状态等非数据库文件不会阻止首次注册。数据库、日志旁文件或已有注册限制的异常状态仍由原门禁判定。

覆盖升级沿用同一数据目录，保留设置、观看历史和续播进度。旧正式版没有 v5 配置或会话，新版首次在线取流时自动准备材料和 SDK 会话。v0.5.7 至 v0.6.5 写入的已知匿名档案，以及此前启动只写入当前版匿名档案的设备，可在没有其他设备记录和注册限制时自动补齐初始注册记录。升级保留现有注册状态与限流记录。

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

## 2026-11-11：官方默认选档与 Tauri 验收

实际短剧入口是 `ShortSeriesImpl → rx4.q$a`，通用 `ResolutionManager` 不是该入口。
点播先尝试离线源，再读手选记忆、同模型已有媒体缓存、动态策略，最后才回退基础 720P。
预取共用手选、缓存和动态策略，但缺档时按允许档位次序查找，与点播按模型支持次序查找有区别。
已有媒体缓存排除 360P，取支持列表中最后一个缓存字节数大于零的档位；同模型保存这次选择。
手选记忆按 `single_progress` / `single_series` 决定进程或持久化、全局或逐剧作用域，持久化读取还受 `show` 约束。

桌面在线选择器已接入六组官方 AB/settings 配置及 `engine_net_report_v649` 的 ShortPlay 码率多项式，
应用服务端建议、规则存活期、条件拦截、高峰降档及集号门槛、网络选档和预取最低档位规则。
首次选档等待策略首包，后续按 `query_gap` 更新，失败保留有效策略与节拍。
“自动”清除当前作用域手选记忆，然后重新决策，不等于最高档，也不等于固定 720P。
本地文件仍由现有本地播放入口处理，不受在线选档覆盖。

证据：`ev_93aed118d25fe29c48edce79325dd4842f1dd34bd2fc6fad4522ac61364bc49d` 是同一隔离用户10、全球死考13集的模型/缓存/原选择器对照；
`ev_ba08ce3511dd3d123434c9a17b0c8f409611d619b9d90c04918066c7835fa8f3` 核对全部配置默认构造与允许档位次序；
`ev_2fe618ff794396b48fb5fac66bcc91438bd4f74967f6ef9ec011e57a05974e6b` 受控调用官方选择器，验证手选全局沿用、服务端升1080P、高峰降480P、集号门槛与 ignore_peak；
`ev_d8e3a0c39a291dc442a5491942d10e92f455abe02f79b6ca8cd095fab5d5dcab` 还验证了存在但为空的高峰配置仍转换成控制器对象，不能误启本地降档。
这些受控调用只临时替换内存策略并在 finally 还原，没有更换身份、清数据或扩大取流窗口。

macOS Tauri 实际验收（同一桌面身份，全球死考）：

| 操作 | 实际输出 |
| --- | --- |
| 第13集自动 | 720P，1280×720，已呈现视频帧 |
| 第13集手选1080P | 1920×1080，接续当前进度，无媒体错误 |
| 切到第14集 | 沿用1080P，1920×1080，已呈现视频帧 |
| 第14集手选360P | ByteVC2直接解码到640×360画布，采样20帧、零丢帧 |
| 第14集恢复自动 | 缓存优先重新选到1080P，确认手选360P已清除 |

原始验收数据为 `target/playback-behavior/tauri-quality-acceptance.json`，窗口图为同目录 `tauri-quality-window.png`，实际视频帧为 `tauri-quality-frame.png`。
无 HEVC 扩展且 ByteVC2 解码器可用时，ByteVC2 档位不再被菜单或手选过滤；该 Windows 能力组合由单测覆盖，本轮实机为 macOS。

平台输入的边界：网络速度来自桌面真实 CDN Range 吞吐样本，不冒充 Android TTVideoEngine/PortraitNetworkScore 的专有预测器；
Wi-Fi 使用当前设备网络快照，Android 的低端/中低端机型名单没有套到桌面硬件。
当前官方样本没有开启网络选档，网络与高峰等非当前自然分支由原字节码及受控对照/回归用例验证。
相关 Rust 库串行测试、前端182项测试、类型检查、生产构建和相关 crate 严格 Clippy 均通过。
ESLint 排除了仓库未跟踪的 `.qa-*` 副本后通过；生产构建仍有既有大包体积提示。
