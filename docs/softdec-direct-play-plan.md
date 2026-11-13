# 内置 HEVC / ByteVC1 软解直播放：调查结果与实施计划

状态：**统一使用内置视频解码。GPUI 在 Windows/macOS 默认内置解码 HEVC / ByteVC1 / ByteVC2，不需要启动声明、环境开关或系统 HEVC 扩展。**

## 当前实现（2026-11-13）

- GPUI 在线播放仅交给内置解码器；不支持的编码明确报错，起播失败不切回系统视频解码或兼容转码。
- 本地 HEVC / ByteVC1 / ByteVC2 使用内置解码；下载记录中的 H.264 或失效文件重新获取在线原流，显式打开不支持的本地文件报错。
- 原生系统视频入口及 AVPlayer 诊断 spike 已删除；系统播放器只承载音轨与时钟。
- Windows 的旧环境开关、`enabled()` 及未使用的平台能力查询接口已删除。
- 共同 service 永久采用内置播放能力；GPUI 启动不再调用 `enable_builtin_playback()`。
- GPUI 的 HEVC 扩展安装提示、安装状态和下载入口已删除；播放页不再引导用户安装系统视频解码扩展。
- Windows 运行效果仍需实机验证；默认启用描述的是实现策略，不代表已完成 Windows 实机验收。

以下 2026-11-11 的实施与调查保留为原 PR 历史。其中“Windows 默认关闭”、环境开关、启动声明和保留 HEVC 安装引导等条目已被上述当前实现取代，不代表现行行为。

基线：`dev` @ `440f523a`（2026-11-11）。调查日期 2026-11-11。

## 历史实施进展（2026-11-11，原分支 `feat/softdec-direct-play`）

### 已做

- 新增 `crates/hongguo-softdec`：不分编码的直播放索引（`IndexedVideo`，由原 `IndexedBytevc2` 迁入并泛化）与 HEVC 解核适配（rusty_h265，版本钉 `=0.7.1`）。ByteVC2 仍走 vvdec。
- `hongguo-bytevc2` 公开找轨、`moov` 读取、vvdec 解码入口与帧缓冲池，供新 crate 复用。
- Tauri 直播放协议端改用新索引，HEVC / ByteVC1 / ByteVC2 走同一条通道。
- 选流新增「只收内置解码器能解的编码」模式：H.264、未标注、未知编码不参赛；整集没有可解的流时报新的错误种类 `error.codecUnsupported`（中英文词条），前端不再对它自动重试。
- 桌面 Tauri 在启动时声明使用内置软解；起播响应带上错误种类。
- 播放页在直播放时不再提示安装 HEVC 扩展。
- dev 构建档给 `hongguo-softdec` 单独开优化（原因与实测见根 `Cargo.toml` 注释）。

### 与计划的出入

- **帧类型与缓冲池留在了 `hongguo-bytevc2`**，没有按 §3.1 移到新 crate：新 crate 依赖它，反过来移会成环。只迁了索引。
- **Android Tauri 在第六轮切到了内置软解**（之前几轮因构建链无法验证而没动）。
- **GPUI 壳只在 macOS 上默认启用**（见第三轮）。Windows 的实现见第五轮，没有在 Windows 上跑过，默认关闭；关闭时是系统播放器，选流不收 ByteVC2（第六轮起播放不再转码）。
- **播放路径的 H.264 兼容代码在第六轮删除**；平台转码层里只服务播放的两样能力（播放码率档、边转边播的渐进输出）还在，见第六轮。
- `dev-habits` skill 仍未安装、未读。

### 第六轮（同日）：Android 切内置软解，删除播放路径的 H.264 兼容代码

**Android 切换**：`hongguo-tauri/src/lib.rs` 去掉 `enable_builtin_playback()` 外面的 Android/iOS 门控，Tauri 壳在所有目标上都声明内置软解。前端播放器本来就不分平台走直播放通道（WebCodecs `VideoFrame` 画画布），没有改。

**Android 构建链**（本机这次有 Android SDK + NDK 29 + arm64 模拟器，第一次能在本地出 APK）。VVdeC 经 NDK 交叉构建从没成功过，这一轮逐个踩出四个障碍，`android.yml` 与 `workflow.md` 都已写上：

1. CMake 的 Android 支持只认 `ANDROID_NDK_ROOT` / `ANDROID_NDK`（只设 `NDK_HOME` 报「Neither the NDK or a standalone toolchain was found」）。GitHub 的 Ubuntu 镜像自带 `ANDROID_NDK_ROOT`，CI 上可能本来碰不到这一条。
2. bindgen 找不到 NDK sysroot 的 C 头（`stdio.h`）→ `BINDGEN_EXTRA_CLANG_ARGS_aarch64_linux_android=--sysroot=…`。
3. VVdeC 生成的 pkg-config 里有 `-l:libunwind.a`（rustc 解析成空库名，「library name must not be empty」）和动态 `libc++`（APK 不带 `libc++_shared.so`）→ `SYSTEM_DEPS_LIBVVDEC_LIB="vvdec c++_static c++abi atomic m dl"`。
4. 构建脚本一重跑（环境变量变化即重跑），cmake crate 传的编译器路径让 CMake 判定「编译器变了」清掉缓存，重配时丢了 Android 目标、按宿主 macOS 探测（给 NDK clang 传 `-arch arm64`）→ 新增 `scripts/android-ndk.toolchain.cmake`（钉 arm64-v8a / API 24 并 include NDK 工具链），经 `CMAKE_TOOLCHAIN_FILE_aarch64_linux_android` 传入。
5. 与本改动无关、但挡住出包的既有问题：`915c3578`（依赖全面更新）把 KGP 与 kotlin-stdlib 升到 2.4.21 后，`buildSrc` 经 AGP 9.3.1 带进来的 KGP 2.2.x 盖掉了根上的 2.4.21，Kotlin 编译报「compiled with an incompatible version of Kotlin」。在 `gen/android/buildSrc/build.gradle.kts` 钉同版本 KGP。最近一次成功的 Android CI 是 11-02，早于这次升级。

**删除的播放兼容代码**（按 §5 清单）：

- service：`play_service::compat_play`（整集/首段兼容转码、取消、离线 QA 本地播放入口）、`ServiceEvent::CompatProgress` 与事件名 `compat-play-progress`；`transcode_service` 只剩合并用的单集转码——去掉播放码率档参数、`TranscodeHooks`、`playback_scale`、`.play.mp4` 缓存名。
- Tauri：命令 `transcode_for_playback` / `cancel_compat_playback` / `qa_offline_local_play` / `playback_capability` / `install_hevc_extension`（`capability_cmd.rs` 整个删掉），事件转发里的兼容进度。
- protocol：`hongguo-protocol::grow`（边写边读的增长文件供数）。
- GPUI：`open.rs` 的 ByteVC2 整集转码分支；没开直播放时（Windows 默认）选流不再收 ByteVC2，`plays_direct` 只在声明内置软解时为真；兜底分支遇到 ByteVC2 报 `CodecUnsupported`。GPUI 的 HEVC 扩展引导（`hevc_prompt`）保留：Windows 不开直播放时它仍然有用。
- 前端：`use-transcode-fallback`、`use-growing-compat`、`growing-media`、`growing-fallback`、`hevc-session`、`compat-policy`（连同测试）、`compat-overlay`、`hevc-install-card`、`PlayingRecoveryOverlays`、离线 QA 本地样本面板与 `/qa-offline-local` 路由；恢复策略去掉「换路兜底」一档（复发直接进错误页）；词条删掉 7 个只给兼容播放用的（`player.compat*`、`player.hevcChooseCompat`），GPUI 还在读的 `player.hevc*` 保留。
- 测试：删掉只测兼容播放的单测与两个 e2e（播放码率档对照、兼容播放转码）。

**保留**：下载、快速合并、兼容合并（`merge_service::compat` → `transcode_service::pipeline::transcode`，平台编码链与 ffmpeg 调试开关）、设置页的转码能力显示与重新检测、HEIC 封面。合并的调用路径只是少传了两个总是默认值的参数，行为不变。

**有意没删、留作后续**：平台转码层（`hongguo-media-platform*` 三个平台实现、`ffmpeg`、`bytevc2_transcode`、`hongguo-media::mux::fragment` 的 `GrowingH264`）里只服务播放的能力——`RateProfile::Playback` 码率档与边转边播的 `on_playable_prefix` / `on_committed` 渐进输出。播放已经不会调到它们，合并也不用，但它们分布在 16 个文件、三个平台实现里，改动面属于另一次结构调整，这一轮没有动。

**Android 运行验证**（本机 arm64 模拟器 `Pixel_9_Pro_XL`，Android 16，**debug** APK，真实在线流；模拟器跑在 M2 Max 的虚拟化上，性能不代表真机）：

- 新装首启按正常流程向官方服务注册了一个新的设备身份（模拟器是一台新设备），之后推荐流自动起播。
- logcat 证据：`[Direct] session=… codec=Bytevc2 720x1280 … decode_ms=419`、`codec=Hevc 1280x720 … decode_ms=181`——ByteVC2（VVdeC）与 HEVC（rusty_h265）都在 Android 上走了内置软解，画面经 WebCodecs 画到画布上（截图可见）。连续上划 5 部剧都起播成功，没有起播失败或解码失败日志。
- 经 WebView DevTools 读画布计数：ByteVC2 720P 1× 30 fps、6 秒 0 丢；2× 显示 56.8 fps，6 秒丢 19 帧；3× 受 60 Hz 封顶（6 秒丢 183 帧）；都无重新缓冲。HEVC 720P 1× 30 fps、0 丢。往后跳 20 秒约 205 ms 落地，之后画面与声音时间一致。
- **没验证的**：真机（任何一台手机）、release 构建、低端 CPU 的解码余量、发热与耗电、横竖屏切换、长时间连播、本地下载件直播放、CI 上的构建（`android.yml` 改了但没触发）。

**测试闸门**：`cargo clippy --workspace --all-targets -D warnings -D clippy::allow_attributes` 通过；`cargo test --workspace --lib -- --test-threads=1` 933 个通过、零失败（减少的是随兼容播放删掉的用例）；`wire_contract` 6 个通过；`pnpm typecheck`、ESLint、Prettier、vitest（153 个）、文件规模闸门通过。knip 的告警与改动前相同，另外删掉了因此变成未用的 `isAndroid`。

### 第五轮（同日）：GPUI Windows 直播放（未验证、默认关闭）与 HEVC 引导收口

**GPUI Windows 直播放**（`crates/hongguo-player-windows/src/direct.rs`）。结构与 macOS 对位：

- 声音与时钟：一个只放声音的 MediaEngine（`player::open_audio`），字节流是同一集的「只剩音轨」视图——`stream.rs` 的字节流泛化成在线流 / 本地文件两种来源（`Feed`），并可挂 `AudioMask`：起播线程建好索引后放行，读到的字节按 `IndexedVideo::mask_video_tracks` 遮掉视频轨。引擎仍在主线程创建；视图的读在索引就绪前等着（读本来就阻塞，跑在 MF 工作线程上）。
- 画面：解码线程把帧按片源口径换成 BGRA（新增 `hongguo_softdec::copy_to_bgra`，定点、环绕运算，4 个测试），放进深度 4 的队列，满了就等。渲染泵挂在 `submit_stage` 上（壳每帧都调），取「显示时间不晚于引擎时钟」的最后一帧，`UpdateSubresource` 写进视频 quad 的 BGRA 纹理，落点几何复用系统播放器的 `geometry`。
- 扣住声音：起播与跳转后引擎先不走，会话首帧上屏后才 `Play`（不改用户的暂停状态），与 macOS 第四轮的做法一致。解码线程不碰引擎与 D3D 对象，只读渲染泵发布的时钟。
- 开关：`hongguo_player::direct_playback_available()` 在 Windows 上读 `HONGGUO_WIN_DIRECT=1`（进程级一次）；不设时选流、起播与原来完全一样。
- **验证只有交叉类型检查**：`cargo clippy -p hongguo-player-windows -p hongguo-player -p hongguo-softdec --target x86_64-pc-windows-msvc -- -D warnings -D clippy::allow_attributes` 通过。为让 vvdec-sys 在 macOS 上交叉通过检查，临时用了 system-deps 的覆盖变量（`SYSTEM_DEPS_LIBVVDEC_BUILD_INTERNAL=never`、`_NO_PKG_CONFIG=1`、`_LIB=vvdec`、`_INCLUDE=<本机内建产物的头文件>`）并让 bindgen 借用 macOS SDK 的 C 头——这只够类型检查，**链接、运行、画面、音画同步、跳转、倍速、换集、设备丢失重建、内存一概没有验证**。已知要在 Windows 上重点看的：只放声音的 MediaEngine 在不 `Play` 时是否照常加载（首帧前时钟停在起点依赖这一点）、`UpdateSubresource` 每帧 8 MB（1080P）的上传开销、窗口最小化时渲染泵停转会让声音一直扣着。

**HEVC 安装引导**。新增 `capability_service::hevc_playback_ready()`：壳声明了内置软解就恒真，否则看系统 HEVC 解码器。Tauri 的 `playback_capability` 命令与 GPUI 的引导条件都改用它。修掉的问题：桌面 Tauri 上非直播放的集（本地 H.264 成品）在没装扩展的 Windows 上仍会弹「装 HEVC 扩展」卡片；GPUI 开了 Windows 直播放后同理。前端的兼容转码与解码失败裁决原本就对直播放短路，没有改。`HevcInstallCard` 组件与 `install_hevc_extension` 命令没删：桌面 Tauri 上已不可达，删除与 §5 的兼容代码清理一起做（被 Android 卡着）。GPUI 的这处改动只是收紧出现条件，没有改布局，`gpui-kit-design-guides` skill 本机没有，`make gpui-preview` 没有跑（macOS 上本来就不出这张卡）。

**Tauri 2× 复测**：本轮机器上有别的项目在跑（虚拟机、Docker、带覆盖率的 vitest，平均负载 7–9），起 Tauri 开发构建本身要重编，会和它们抢资源，**没有做，仍未定论**。

**测试闸门**：`cargo test --workspace --lib -- --test-threads=1`（与 CI 同口径）952 个通过、零失败。顺带记录：不加 `--test-threads=1` 时 `hongguo-service` 的设备生命周期测试与下载临时文件测试会成批失败（14 个），原因是几个既有测试直接改进程环境变量（`HONGGUO_OFFLINE_QA`、`HONGGUO_REUSE_DEVICE_JSON`）且共享进程级状态；单独跑或单线程跑都通过，与本改动无关。`hongguo-api` 的 `invalid_proxy_url_errors` 在并行全量跑时也失败过，单独跑通过。

### 第三轮（同日）：GPUI 壳 macOS 内置软解直播放

**做法**（与 §3.3 的方案一致，取了「AVSampleBufferDisplayLayer」那一支）：

- 声音与时钟：仍是一个 AVPlayer，喂同一集的「只剩音轨」视图（`loader::Source::Audio`，复用 `IndexedVideo::mask_video_tracks`）。暂停、倍速、跳转、播完、缓冲事件都沿用原系统播放器的实现。
- 画面：`AVSampleBufferDisplayLayer` 叠在原播放层上（宿主视图的 overlay），时钟直接绑到 AVPlayerItem 的时间基，音画同步由系统保证。解码帧（I420）在 `hongguo-softdec` 里拷成 NV12，挂上码流自带的色彩口径（BT.709 等）作为像素缓冲附件，由系统做换算与色彩管理——观感与系统播放器一致，不需要 Tauri 那条路的 BT.601 归一。
- 起播与跳转时先扣住声音，第一帧送进显示层后再放行，避免时钟跑在解码前面。
- 建索引要读网络，放在 `softdec-open` 线程；GPUI 主线程不等它。
- 新增门面 `hongguo_player::open_direct` 与 `direct_playback_available()`（只有 macOS 为真）。`PlaySource` 与 Windows 播放器 crate 一行没动：本机编译不到 Windows，改契约就要盲改那边的穷尽匹配。
- GPUI 启动时在有直播放的目标上声明内置软解；起播链对 HEVC / ByteVC1 / ByteVC2 走直播放（本地成品按文件实际编码判断），不再整集转码。Windows 上 `direct_playback_available()` 为假，行为与之前相同（**未在 Windows 上编译或运行**）。
- 新增取证开关 `HONGGUO_AV_SCRIPT=+20,-15,pause,resume,def:720,def:auto`（首帧后每 3 秒执行一步：相对跳转、暂停、恢复、切清晰度，每步打当前位置）；`HONGGUO_AV_METRICS=1` 时直播放每 5 秒打一行上屏帧率、丢帧、落后量、时钟位置与走速。

**运行验证**（macOS arm64，GPUI **开发构建**，真实在线流）。GPUI 在本机从没运行过，正常首启会建新库并向官方服务注册一个新的设备身份；为避免这一步，测试时用 `HONGGUO_DATA_DIR` 指向 Tauri 数据目录的**临时副本**（沿用已有身份，不碰你的真实数据）。

| 项                                      | 结果                                                                                    |
| --------------------------------------- | --------------------------------------------------------------------------------------- |
| ByteVC1 1080P，1× / 2× / 3×             | 每 5 秒窗口稳定 30 / 60 / 90 fps，丢帧 0，最大落后 ≤ 41 ms；进程 CPU 约 30% / 57% / 71% |
| ByteVC2 720P，3×                        | 16.6 秒上屏 1464 帧（约 88 fps），丢帧 0                                                |
| 跳转 +20 / −15 / +30 秒（扣住声音之后） | 首帧时间戳与目标一致，出画 78–222 ms，丢帧 0，落后 0                                    |
| 扣住声音之前的同一测试                  | 跳转后落后 312–462 ms、丢 11–19 帧——这是加「扣住声音」的依据                            |
| 冷启动首帧                              | 约 1.2–2.1 s（含建索引、音轨 item 加载与取数）；改成解码与 item 加载并行之前是 4.1 s    |
| 播完自动切下一集                        | 首帧 194–323 ms                                                                         |
| 画面                                    | 截屏确认解码画面在窗口里、弹幕叠在上面、颜色正常（截屏含你的其他窗口，已删除，未保存）  |
| 内存                                    | RSS 约 180–470 MB                                                                       |
| 没走旧路径                              | 日志里每集都是 `走内置软解直播放` 与 `[Direct] … codec=Hevc/Bytevc2`，没有兼容转码记录  |

**第四轮补测（GPUI，同一套环境，用 `HONGGUO_AV_SCRIPT` 走与用户操作相同的入口）**：

| 项                           | 结果                                                                                                                                                                                                                               |
| ---------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 暂停                         | 时钟停住（41.5393 → 41.5395 秒，6 秒内），解码线程因显示层队列满而自然停下                                                                                                                                                         |
| 恢复                         | 继续 30 fps，丢帧 0                                                                                                                                                                                                                |
| 切清晰度 1080P → 720P → 自动 | 同位重播（57.098 → 57.098 秒），720 那一档恰好是 ByteVC2，也走直播放；出画 260 / 549 ms                                                                                                                                            |
| 本地成品文件                 | 在测试库副本里登记了「已下载」记录，指向本机合成的 90 秒 720P HEVC + AAC 文件（不是真实剧集）：判定 `direct=true`，首帧 311 ms，29–30 fps；暂停、跳到 89 秒播完、自动切到下一集（同样走本地文件，首帧 125 ms）、往回跳，丢帧均为 0 |
| 3× 在机器重负载下            | 平均负载 34–38 时解码跟不上 90 fps，按设计丢帧追时钟（5 秒窗口丢 200 多帧），声音不停、不卡死。第一次看到时误以为是暂停的问题，加了时钟走速诊断后确认是 3×（取证用的倍速被 GPUI 记住了）                                           |

**发现但没修的边角**：起播点超过文件时长时（只在拿 2 秒的小样本冒充一集时出现），音轨播放器起播即到尾，之后往回跳也不再走，`Ended` 也不会发。真实下载件与在线流的时长一致，正常使用碰不到；没有深究是 AVPlayer 在尾部的状态问题还是本模块的问题。

**Windows**：装了 `x86_64-pc-windows-msvc` 目标做交叉类型检查——`hongguo-player-api` / `hongguo-player-windows` / `hongguo-player` 通过。整个 GPUI 壳和 Tauri 壳检查不了：GPUI 自己的构建脚本在 macOS 上交叉到 Windows 就失败，VVdeC 也需要 MSVC。GPUI 起播链里我改的代码不分平台，与 macOS 上编译通过的是同一份。（第三轮时 Windows 直播放还没写；第五轮补上了实现，仍未运行验证，见上。）

**Android**：`android.yml` 的构建步骤补上了 `SYSTEM_DEPS_LIBVVDEC_BUILD_INTERNAL=always` 与 `PKG_CONFIG_ALLOW_CROSS_aarch64_linux_android=1`（调查时发现的第一个确定障碍）。没有触发 CI，VVdeC 经 NDK 交叉编译能否成功、bindgen 能否找到 Android 头文件都**未知**。Android Tauri 仍未切到内置软解。

**仍然没测到的（GPUI）**：弱网、长时间连播、窗口关闭时的资源释放、发布构建、Intel Mac 运行（只做了交叉 clippy）、真实剧集的本地下载件。

**Tauri 2× 复测**：这一轮机器负载始终在 18–38 之间，没有干净条件，**仍未定论**。

**Tauri 复测**（第二轮欠下的）：负载降到约 16–17 后，1080P 1× 为 30 fps、零丢帧、偏差 2 ms；2× 无重新缓冲，但每秒约跳过 8 帧（显示 52 fps）；3× 显示受 60 Hz 封顶，无重新缓冲。2× 的跳帧在第一轮（机器更空闲、还没有色彩换算）没有出现，这次负载仍偏高，没能分清是换算、负载还是 60 Hz 下的时钟抖动，**未定论**。

### 第二轮（同日）：画面对拍、色彩、内存、改名

**画面对拍。** 在运行中的应用里，把同一时间点的内置软解画面（直播放画布）与系统解码器画面（WebView 自带 HEVC 解码的 `<video>`）都缩到 480×270 逐像素比较，真实 ByteVC1 1080P，取了多个时间点：

| 步骤             | 同一帧的 PSNR | 说明                                                                      |
| ---------------- | ------------- | ------------------------------------------------------------------------- |
| 最初             | 约 25 dB      | 画面结构对得上（换成 2 秒后的帧只有 10.6 dB），但有系统性的亮度与颜色偏差 |
| 归一色彩口径之后 | 约 30 dB      | 三通道偏差变得一致（约 −7），只剩一条纯伽马曲线                           |
| 再扣除伽马 0.90  | 39.5 dB       | 剩余误差来自两边不同的缩放与取整                                          |

结论：**解码内容是对的，差异全部来自显示换算。** 具体有两层：

1. WKWebView 画 `VideoFrame` 时固定按 BT.601 全范围换算，传给它的 `colorSpace` 完全不看（用已知色值标定：`Y=16` 画出来是 16，BT.601 的纯红码值画出来是纯红，四种声明结果一模一样）。真实 ByteVC1 码流在 SPS 里声明的是 BT.709 限幅范围，所以原样送过去会发灰、色相略偏。**这个问题在原有的 ByteVC2 直播放里一直存在。** 现已修：出帧时把 YUV 归一成 BT.601 全范围（`hongguo-softdec` 的 `FrameColor::convert_to_bt601_full`），前端如实声明。
2. 剩下的伽马 0.90 是 macOS 对系统视频做色彩管理的结果（把 BT.709 传递函数当作约 1.96 的伽马，即常说的 QuickTime 伽马偏移）。内置软解的画面不经过这一层，观感接近 Chrome / VLC，比系统播放器的中间调略深。**这一层没有去模仿**，是否要对齐系统观感需要你决定。

ByteVC2 的解码器绑定不暴露色彩标记，口径是按惯例推定的（限幅范围，高清用 BT.709）。用 ByteVC2 720P 的软解画面与同一时刻系统解码的 ByteVC1 1080P 画面比较：最佳伽马同样是 0.90，扣除后三通道偏差约为 0（0.35 / 0.31 / −0.04），再做颜色拟合只多 0.2 dB，说明推定成立。

**内存。** 根因不是批量大小，而是响应缓冲边装边翻倍扩容：8 帧 1080p 要经过 3→6→12→24 MB 四次大块分配，macOS 分配器把释放的大块留着不还。改成拿到第一帧后一次分配到位，播放 1080P 时物理占用从峰值约 1.8 GB 降到约 265 MB（峰值 400 MB）。中途试过把每批限制在 8 MB 以内，内存同样降下来但每批只剩 2 帧，没有采用。

**改名。** 协议路径 `/bvc2/` → `/direct/`；响应字段 `needsCompat` → `direct`；Rust 模块 `direct_bytevc2*` → `direct_video*`；前端 `direct-bytevc2` / `isBytevc2` / `DirectBytevc2Player` 等 → `direct-video` / `isDirect` / `DirectVideoPlayer`；画布标记 `data-bytevc2-direct` → `data-direct-video`；Windows 共享缓冲的元数据键 `bytevc2Session` → `directSession`（Rust 与前端两侧同时改，Rust 那一侧在 Windows 专属代码里，**本机编译不到**）。`scripts/bytevc2-playback-audit.mjs` 的选择器与事件名已跟着改，文件名未改。

**性能。** 色彩换算在优化构建里约 0.65 ms 一帧（按 CPU 时间：解码 + 打包 8.6 ms，加上换算 9.2 ms，单核约 108 fps）。第一版换算在开发构建里因为整数溢出检查没能向量化，吃掉解码线程约两成时间；已改成显式环绕运算，开不开溢出检查都是同一速度，输出逐字节不变。

**这一轮没能验证的：** 加入色彩换算之后的 1×–3× 实际播放没有在干净条件下复测——测的时候这台机器被别的进程占满（平均负载 25–60），同一时刻 2×、3× 出现重新缓冲，而同时测得的每帧实际耗时是 CPU 时间的 3–4 倍，所以这组数字不能用来判断换算是否拖慢了播放。1× 在这种负载下仍是 30 fps、零丢帧。

### 第一轮的真实运行验证（macOS arm64，Apple M2 Max，Tauri **开发构建**）

用应用自身的在线取流播放《糯糯下山，师兄们都慌了》，没有另行抓取或保存样本。

| 项                        | 结果                                                                                                                                             |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| ByteVC2 720P（自动选档）  | 经新 crate 出画，首帧 227–551 ms；约 17 秒内显示 525 帧、丢 1 帧，音画时间一致                                                                   |
| ByteVC1 1080P（手动切档） | 日志 `codec=Hevc 1920x1080`；8 秒 240 帧（30 fps）、零丢帧、无解码错误                                                                           |
| 拖动到 80 / 20 / 100 秒   | 出画时间戳与目标完全一致；耗时 3.06 / 0.48 / 0.81 秒（**优化 dev 档之前**测的，之后未重测）                                                      |
| 1× / 2×                   | 30 / 60 fps，零丢帧、零重新缓冲，音画偏差 16 / 31 ms                                                                                             |
| 3×                        | 音频按 3 倍推进、零重新缓冲；显示 59.5 fps（受 60 Hz 刷新率限制，6 秒内跳过 183 帧属预期）                                                       |
| 没走旧路径的证据          | 播放期间日志无任何转码、ffmpeg、系统解码会话的记录；`compat-cache/` 目录不存在。启动时有一行「平台编码层可用」，那是合并功能的能力预热，不是播放 |

优化 dev 档之前，同一构建 1× 就占满一个核、2× 持续重新缓冲；采样显示解码线程约 63% 的时间在出帧打包循环上。

**内存**：播放 1080P 时进程物理占用峰值约 1.8 GB。`vmmap` 显示其中约 1.2 GB 是已释放、被系统分配器暂留的大块（62 个 23.7 MB 的区域，对应非 Windows 传帧路径每批 8 帧的响应缓冲），存活的大块只有约 54 MB。这不是泄漏，但占用偏高。第二轮已处理，见上。

### 自动化验证

- `cargo test --workspace --lib -- --test-threads=1`：全部通过（第二轮结束时 945 个、第五轮结束时 952 个，零失败）。两轮共新增约 50 个测试，其中 `hongguo-softdec` 24 个——合成 HEVC 样本的全部帧与 ffmpeg 解码结果逐字节一致（8-bit 与 10-bit）、B 帧显示序、关键帧跳转后画面与从头解码一致、取消与读错误传播、H.264 与无音轨被拒、色彩换算的标准色值与 2×2 色度对应关系。
- `hongguo-signer` 有一个按毫秒时间戳比较的既有测试偶尔失败（第一轮遇到过一次，重跑通过），与本改动无关。
- `cargo clippy --workspace --all-targets -- -D warnings -D clippy::allow_attributes`：通过。
- 文件规模闸门、`pnpm typecheck`、ESLint、Prettier、vitest（182 个）：通过。

### 仍然没有的证据

- **ByteVC1 的画面只做了缩小后的统计对比，没有做全分辨率逐像素对拍，也没有人眼目检。** 第二轮的对比能排除解码花屏、错帧、偏色这类问题，但 39.5 dB 的上限来自缩放，不足以证明逐比特一致；要做到那一步需要落地的真实样本文件。只比了一部剧的几集。
- **发布构建、Windows、Intel Mac、Android 一律没有运行验证。** Windows 的传帧走另一条路径（WebView2 共享缓冲），本次改动没有碰它，但也没有在 Windows 上跑过。
- 低配 CPU 上的表现未知。本机发布档实测 `hongguo-softdec` 端到端（解码 + 打包）1080p 合成片段约 144 fps。
- 只提供 H.264 的集只用构造的流表测过，没有遇到真实案例。
- 弱网、长时间连播、本地文件直播放、多窗口都没有测。

本文把结论分成四种可信度，请按标注阅读：

- **【已核实】** 我直接读过代码或在本机实测过。
- **【代理报告】** 由只读调查子任务读代码得出，附了路径行号，我抽查了其中决定方案走向的部分，但没有逐条复核。
- **【推断】** 从代码或记录推出来的，没有运行证据。
- **【待验证】** 目前没有证据，实施时必须先验证。

---

## 0. 强制阅读的完成情况

| 要求                                          | 结果                                                                                                                                                                                                                                           |
| --------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 用户级 `dev-habits` skill                     | **未读到。** 本机 `~/.agents/skills/` 与 `~/.claude/skills/` 里都没有它（只有 `ego-browser`）。AGENTS.md 给的补装命令是 `npx skills add it00021hot/skills@dev-habits -g`，这会从第三方仓库下载并安装内容，我没有擅自执行，需要你决定是否安装。 |
| `.agents/skills/hongguo-dev/SKILL.md`         | 已读全文。                                                                                                                                                                                                                                     |
| `references/backend.md`、`workflow.md`        | 已读全文。                                                                                                                                                                                                                                     |
| `references/gpui.md`、`ipc.md`、`frontend.md` | 读了开头与结构；实施对应步骤前需要通读。                                                                                                                                                                                                       |
| `AGENTS.md`                                   | 已读全文。                                                                                                                                                                                                                                     |
| `gpui-kit-design-guides` skill                | **本会话不可用**（不在可调用的 skill 列表里）。计划中凡是改 GPUI 可见界面的步骤（见 §6.4）在它可用之前不能动手。另外 AGENTS.md 写的名字是 `gpui-ui-design`，SKILL.md 写的是 `gpui-kit-design-guides`，两处不一致。                             |

---

## 1. 调查结果

### 1.1 现有播放链路

**Tauri 壳**【代理报告，关键点已核实】

- 取源命令 `play_series` → `play_service::local::resolve_play`。前端 `usePlay` 固定以 `filePath=''、preferOnline=true` 调用（`src/service/queries/play.ts:22`，已核实），所以 Tauri 播放页对已下载的集也走在线流；`resolve_play` 的本地分支在当前界面里没有入口。
- 非 ByteVC2 流（含 HEVC / ByteVC1 / H.264）：`<video src="hongguo-stream…/s/{def}/{vid}">`，由 WebView 的系统解码器解。
- ByteVC2：已经是**进程内软解直播放**（下节）。
- HEVC 解不出来时的兜底：整集转成 H.264 再播（`compat_play` → `transcode_for_playback`，前端 `use-transcode-fallback` + MSE `use-growing-compat`）。Windows 上先弹“安装 HEVC 扩展”卡片。

**GPUI 壳**【代理报告，关键点已核实】

- 只走系统播放器：macOS 是 AVPlayer + AVPlayerLayer（经 `AVAssetResourceLoader` 喂解密后的明文 MP4），Windows 是 Media Foundation MediaEngine frame-server（经自定义 `IMFByteStream` 喂明文，画面写进 vendored `gpui-pre-windows` 的 `video_quad` 纹理）。
- 唯一的编码分支在 `crates/hongguo-gpui/src/home/playback/open.rs:160`（已核实）：ByteVC2 先**整集**转成 H.264 再当本地文件播；其余编码直接交给系统播放器。
- **没有任何把应用自带原始帧显示出来的现成生产路径。** macOS 完全没有；Windows 的 `VideoQuad` 接口本身与 MediaEngine 无关，但只收 BGRA 纹理，目前只有 MediaEngine 在用。
- 历史上有过自研引擎 `crates/hongguo-playback`（VT/MF 解 HEVC + `surface()`/`img()` 上屏），2026-10-23 在 `e0aedf19` 删除，理由是系统播放器已全面接手。它实现过“起点前最近关键帧”“按 dts 水位重排”等逻辑，可作参考。
- GPUI 壳没有 Android 目标。

**ByteVC2 直播放的实际路径（Tauri）**【代理报告】

1. 索引：`hongguo_bytevc2::IndexedBytevc2::open` 用回调逐盒读 moov（头尾都能找到），取视频轨样本表和 `bv2C`，要求存在 `mp4a` 音轨。
2. 供数：在线走 `hongguo_stream::read_range_cancellable`（与 `<video>` 共用同一套等待、解密和按需优先填充）；本地走文件读。
3. 解码：每个会话一个 `vvdec::Decoder`（线程数取核数、封顶 8），从目标前最近的 stss 关键帧解起，丢掉目标之前的帧。输出 8-bit I420（10/12-bit 右移到 8-bit）。
4. 传帧：专用解码线程 + `sync_channel(16)` 背压；每批最多 8 帧。Windows 用 WebView2 共享缓冲，其余平台（macOS、Android）把帧放进 HTTP 响应体。线格式是“宽、高、PTS + Y/U/V”。
5. 渲染：前端用 WebCodecs `VideoFrame`（`format: 'I420'`）画到 2D canvas；YUV→RGB 由浏览器完成，**代码没有传色彩空间**。
6. 音频与同步：同一个 `<video>` 元素（设为透明）播放“视频轨被改名为 `free`”的原 MP4，即只剩 AAC；它的 `currentTime` 是主时钟，rAF 里按时钟挑帧，跟不上就丢帧，欠帧时暂停音频进入缓冲。
7. 倍速：完全靠 `<video>.playbackRate` 推时钟，解码侧没有任何配合。
8. 打包：vvdec 由 `vvdec-sys` 自带的 VVdeC 3.0.0 源码经 CMake 编成静态库，链进主二进制。**没有任何 `target_os` 门控，Android 也会编它。**

**这条路径里哪些与编码无关**【代理报告】：会话代次与取消、协议路由、Origin 校验、帧线格式、共享缓冲、音轨遮罩、背压，以及整个前端播放器 `DirectBytevc2Player`。绑死 ByteVC2 的只有：轨道校验（`is_bytevc2`）、`bv2C` 解析、`vvdec` 调用，以及一批命名（`/bvc2/`、`bytevc2Session`、`needs_compat`）。

**H.264 转码的全部调用点**【代理报告】

| 调用点                                                                     | 所属功能                     | 属于播放路径       |
| -------------------------------------------------------------------------- | ---------------------------- | ------------------ |
| `compat_play::compat_for_playback_prefix` ← Tauri `transcode_for_playback` | Tauri HEVC 解不出时的兜底    | 是                 |
| `compat_play::compat_for_playback` ← GPUI `open.rs`                        | GPUI 播 ByteVC2              | 是                 |
| `compat_play::qa_offline_local_play`                                       | 离线 QA 本地样本             | 是（仅 QA 开关下） |
| `merge_service::compat::compat_merge` → `pipeline::transcode(Archive)`     | **兼容合并**（导出通用格式） | 否                 |
| 下载                                                                       | 不转码，只取流、下载、解密   | 否                 |
| 封面 HEIC → JPEG                                                           | 不是 H.264                   | 否                 |

外部 ffmpeg 进程默认关闭（需显式设 `HONGGUO_ALLOW_FFMPEG`），且只在兼容转码里可达；直接播放路径不会调用它。

### 1.2 媒体与数据

**能否复用给 HEVC 软解**【代理报告】

- 样本表：`stsz/stco/co64/stsc/stts/ctts(v0,v1)/stss/mdhd/elst(首个 edit)` 都已解析；`TrackInfo` 提供 `samples`、`sample_pts()`、`max_reorder_secs()`、`stss`。关键帧 seek 和显示时间轴够用。
- `hvcC`：`hongguo_media::hevc::param_nalus_from_hvcc` 取参数集与 NAL 长度字节数，`to_annexb` 做长度前缀→Annex-B。**没有 SPS 比特流解析。**
- CENC：按整样本 AES-CTR 解密（8 字节 IV，不处理 subsample），支持任意明文区间按需解密；去保护时把 `encv` 改回 `frma` 里的原 fourcc。
- 渐进 Range：解码器可以在流还在填充时读任意明文样本区间，现有 ByteVC2 直播放就是这样用的。

**缺口**

- `sample_dts()` 公开接口在 `0017713f` 被当作死代码删掉了；如果解码器不自己做输出重排，需要恢复它。rusty_h265 的 `next_frame()` 文档写明按输出顺序出帧【已核实，见其 README】，所以大概率不需要。
- 不解析 `sdtp`、多 `stsd` 入口、多个 edit、fMP4。
- `hev1`（参数集在码流内）没有测试覆盖。

**真实样本与观测**

| 类别                                         | 现状                                                                                                                                                                                                                                                        |
| -------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 代码能识别的编码串                           | `h264 / avc1 / h265 / h265_hvc1 / hevc / hvc1 / bytevc1 / bytevc2 / bvc2`（`stream_pick/mod.rs:24-33`，已核实）                                                                                                                                             |
| 服务实际返回的编码（文档与提交记录里的观测） | 只观测到 **`bytevc1` 与 `bytevc2`**。例如 `docs/req-hg-003-playback-parity.md` 记“5 档 ByteVC1”，提交 `e086b951` 记同一部剧不同集的 720P 分别是 bytevc2 和 bytevc1。**没有任何真实记录显示服务返回过 `h264` 或 `h265`**，它们只出现在合成测试数据里。       |
| 仓库内的真实媒体                             | **没有任何视频样本。** 只有一张 HEIC 封面、一份 445 字节的 `bv2C` 配置、两段 AAC ADTS。                                                                                                                                                                     |
| 仓库外被测试引用的样本                       | `/tmp/hg-bytevc2-plain.mp4` 等，本机都不存在。文档引用的 `target/playback-behavior/` 等实测记录本机也不存在。                                                                                                                                               |
| 真实样本可播放的证据                         | ByteVC1 被 **VideoToolbox**（旧自研引擎，`d9662656`）和系统播放器解出过；ByteVC2 被 vvdec 解出过（文档记录）。**没有记录明确说“ByteVC1 被 rusty_h265 或 libde265 解过”**：rusty_h265 历史上在“1080p 真实码流 3611 帧”上跑通且帧数一致，但那次没写编码标签。 |

所以目前能说的是：ByteVC1 在本项目里一直被当作标准 HEVC 交给系统解码器，并且确实能解【有记录】；它能否被我们要内置的软解器正确解出，**没有直接证据**【待验证】。还有一条历史异常需要留意：已删文档记录过“ffmpeg 软解前段损坏/粉块而 VT 正常”，根因未查明。

### 1.3 Android 的真实情况【代理报告】

- 只有 Tauri 壳出 arm64 APK，仅手动触发 CI。**全部历史只有 1 次成功构建（2026-11-02），早于 vvdec 接入（11-10）。**
- `android.yml` 没有设置 vvdec 源码构建需要的 `SYSTEM_DEPS_LIBVVDEC_BUILD_INTERNAL`，按默认行为很可能直接构建失败；VVdeC 用 NDK 交叉编译从未验证过。
- 只有模拟器上的协议层证据，**没有真机播放成功的记录**；workflow.md 自己也写着“未真机验证”。
- 本机没有 Android SDK/NDK，也没有设备。**Android 我在这里既不能构建也不能运行验证。**

### 1.4 代码与文档的差异（以代码为准）

- `AGENTS.md` 写 `hongguo-bytevc2`“不解帧，解码交给 ffmpeg”，实际已内置 vvdec。
- `PlayResponse.needs_compat` 字段名是历史遗留，实际含义是“走 ByteVC2 直播放”。
- `THIRD-PARTY-NOTICES.md` 的编解码一节写“均为纯 Rust 实现，不含 C 代码”，`tauri.conf.json` 的描述写“纯 Rust 零外部依赖”，与 vvdec（C++）不符。
- `Makefile` help 列了 MSI / DEB / AppImage，实际只出 NSIS / DMG / APP；没有 Linux 目标。
- 设置页文案 `ffmpegInstallHint` 说装上 ffmpeg 后“转码自动改走它”，实际默认被开关挡住。
- `stream_pick` 注释说无 HEVC 机器遇到纯 HEVC 集“靠转码兜底”，但选档逻辑可能在此之前就报“没有可播放的清晰度档位”【推断，无测试覆盖】。
- `backend.md` 写“别再指望纯 Rust 软解”。那是针对**整集转码**（软解 + 软编，几十秒一集）下的结论；本计划是只解不编的实时播放，场景不同，但这条历史结论应在评审时一并看到。

---

## 2. 解码器选型

### 2.1 候选

|                    | rusty_h265 0.7.1                                                                     | libde265 1.1.x                                          |
| ------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------- |
| 形态               | 纯 Rust，无 C、无 build script                                                       | C++，需 CMake 或 cc 构建                                |
| 许可证             | Apache-2.0【已核实，crate 自带 LICENSE】                                             | LGPL-3.0【搜索结果一致，采用前需对照上游 COPYING 复核】 |
| 是否已在依赖树     | **是。** `crates/hongguo-media/Cargo.toml` 已依赖它做 HEIC 封面解码                  | 否，需新增依赖与绑定 crate                              |
| 支持范围           | Main / Main 10 / Main Still Picture，4:2:0，8 和 10 bit；Range extensions 等明确拒绝 | 更全（含高位深、Range Extensions）                      |
| 多线程             | **没有**（单线程）【已核实，源码无线程原语】                                         | 有工作线程                                              |
| SIMD               | x86-64 AVX2 运行时分发 + aarch64 NEON【已核实】                                      | 有 x86 SIMD；ARM NEON 情况未查到                        |
| 交叉编译 / Android | 随 Rust 目标直接编，无额外工具链                                                     | 需要 NDK C++ 交叉编译，与 vvdec 同类风险                |
| 成熟度             | 很新（0.7.0 于 2026-11-07 发布），单一维护方，版本间有破坏性改动                     | 多年，被 libheif 等广泛使用                             |
| 已知问题           | debug 构建下去块滤波有一处过严断言，仓库已用 profile 关掉该依赖的 debug 断言         | 历史上有较多安全修复发布                                |

### 2.2 本机实测（rusty_h265，只解码）【已核实】

设备 Apple M2 Max，release 构建，单线程。片段是本机 x265 生成的**合成内容**（10 秒 30fps，约 1.2 Mbps，含 B 帧），不是真实剧集，更不是 ByteVC1。

| 片段                           | rusty_h265 | ffmpeg 原生 hevc 单线程 | ffmpeg 多线程 |
| ------------------------------ | ---------- | ----------------------- | ------------- |
| 1080p 8-bit（mandelbrot）      | 115.6 fps  | 约 169 fps              | 约 667 fps    |
| 1080p 8-bit（testsrc2 + 噪声） | 156.8 fps  | 约 224 fps              | 约 682 fps    |
| 1080p 10-bit                   | 123.8 fps  | 未测                    | 未测          |
| 720p 8-bit                     | 240.5 fps  | 未测                    | 未测          |

四个片段解码错误数均为 0，帧数与输入一致。这只说明：在这台机器上，对标准 HEVC，1080p 30fps 有约 4–5 倍余量，3 倍速（需 90 fps）仍有余量。**它不说明低端 x86、手机，或真实 ByteVC1 码流的表现。**

### 2.3 推荐

**推荐路线 A：用仓库已有的 rusty_h265 作为 HEVC / ByteVC1 解码核。**

- 理由：已在依赖树里、许可证与项目兼容且无再链接义务、三端构建零新增工具链（Android 尤其重要）、标准 HEVC 实测够快。
- 这条路线**不新增第三方依赖**，只是把它的用途从“HEIC 封面”扩到“视频播放”。是否算“引入新依赖”请你裁定；若算，这里就是征求同意的地方。
- 主要风险：单线程。低端 CPU 上 1080p 可能达不到实时，高倍速更紧。必须在阶段 0 用真实样本和目标设备实测，不达标就按 §2.4 处理。
- 次要风险：库很新。阶段 0 要做正确性对拍（见 §7.1），并把版本用 `=0.7.1` 钉住。

**备选路线 B：libde265。** 仅当阶段 0 证明 A 在目标设备上性能或正确性不够时启用。代价是 LGPL-3.0 的分发义务（静态链接进闭源许可的应用需要提供可再链接的手段或改为动态链接）、新增 C++ 构建链（Windows / Intel Mac 交叉 / Android NDK），以及一个新的绑定依赖。**这些都需要你另行确认，本计划不默认采用。**

**不采用**：外部 FFmpeg 进程（你已排除）；进程内链接 libavcodec（体量与许可证代价都明显更大，没有证据表明必要）。

### 2.4 性能不达标时的处理（需要你定原则）

可选做法，互不排斥：

1. 选档时把软解吞吐作为约束，自动选较低清晰度，并在界面明示原因。
2. 限制可用的最高倍速并明示。
3. 切到路线 B。

哪一种可以接受需要你决定；我不会静默降级。

### 2.5 专利

rusty_h265 与 libde265 的许可证都不含专利授权；HEVC 与 VVC 都有专利池。仓库对 vvdec 的记录是“不含专利授权；产品已确认可嵌入发版”。内置 HEVC 软解并随二进制分发，是否需要同样的确认，我无法判断，**需要你或法务核查**。这是我能给出的全部，不构成法律意见。

---

## 3. 架构方案

### 3.1 模块归属

新增一个 crate **`hongguo-softdec`**（名字可改），放“软解直播放会话”，两壳共用。需要它的原因是：现在这层逻辑一半在 `hongguo-bytevc2`、一半在 `hongguo-tauri`，而 GPUI 的播放器 crate 不能依赖 Tauri 壳。上游文档 `req-hg-003` 的“GPUI 直播放迁移计划”第 1 条也是这个方向。

```
hongguo-video（样本表 / CENC）   hongguo-media（hevc 工具 + rusty_h265）   hongguo-bytevc2（bv2C + vvdec）
            └───────────────────────────┬──────────────────────────────────┘
                                 hongguo-softdec（新）
        索引（moov、关键帧 seek、音轨遮罩）· 解码核分派（HEVC | ByteVC2）
        有界帧队列 · 取消 · I420 帧与平面池
            ┌───────────────────────────┴──────────────────────────────────┐
   hongguo-tauri（协议路由 + 帧交付）                    hongguo-player-macos / -windows（原生上屏）
```

依赖方向已核对，不成环：`hongguo-softdec` 只向下依赖 `common / video / media / bytevc2`，不依赖 `stream`（读数据用回调注入，和现在一样）。

各部分归属：

| 职责                             | 归属                                                                                                                              | 说明                                |
| -------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------- |
| 解复用与样本表                   | `hongguo-video`（不动）                                                                                                           |                                     |
| hvcC / Annex-B                   | `hongguo-media::hevc`（不动或小补）                                                                                               |                                     |
| bv2C / vvdec                     | `hongguo-bytevc2`（保留解核与配置解析）                                                                                           |                                     |
| 索引、seek、音轨遮罩、会话、帧池 | 从 `hongguo-bytevc2::{indexed,container 通用部分,decode 的帧与池}` 和 `hongguo-tauri::direct_bytevc2*` **移到** `hongguo-softdec` | 纯移动与行为修改分开提交            |
| HEVC 解核适配                    | `hongguo-softdec` 内新模块                                                                                                        | rusty_h265 的 u16 平面 → 8-bit I420 |
| Tauri 协议与帧交付               | `hongguo-tauri`（保留，改成不分编码）                                                                                             |                                     |
| GPUI 原生上屏                    | `hongguo-player-macos / -windows`                                                                                                 | 见 §3.3                             |
| 选流与能力                       | `hongguo-api::stream_pick`、`hongguo-service`                                                                                     | 见 §4                               |

两种解码核用一个枚举分派即可，不引入 trait 体系。

### 3.2 Tauri：WebView / Canvas 路径

沿用现有 ByteVC2 直播放的全部机制，只做“去 ByteVC2 化”：

- 协议子路径从 `/bvc2/` 改为与编码无关的名字（如 `/direct/`）；会话、批次、背压、取消不变。
- `PlayResponse`：现有 `needsCompat` 实际表示“走直播放”。计划把它改名为表达真实含义的字段（如 `direct`），并同步 Rust 模型、zod schema、调用方。这是 IPC 契约变更，按 `ipc.md` 的全链路清单执行。
- 前端 `DirectBytevc2Player` 改名并去掉 `isBytevc2` 闸；逻辑不变。
- 数据跨边界：Windows 继续用 WebView2 共享缓冲（零拷贝到 JS）；macOS / Android 继续用二进制响应体。队列仍有界（Rust 侧 16 帧，JS 侧低水位 16 帧）。

### 3.3 GPUI：原始帧渲染路径

GPUI 现在没有显示原始帧的能力，这是全计划里**工作量最大、不确定性最高**的部分。

- **音频与时钟**：两端都让系统播放器继续播“只剩音轨”的 MP4（复用现有 ResourceLoader / IMFByteStream，在供数处套用音轨遮罩），以它的播放位置为主时钟。seek、倍速、暂停仍由系统播放器执行。
- **macOS 画面**【待验证】：在现有宿主 NSView 里把 AVPlayerLayer 换成一个普通 CALayer，按时钟把 I420 转成 NV12 `CVPixelBuffer`（可复用 `hongguo-media-platform-macos` 的缓冲池）后设为 layer 内容；或用 `AVSampleBufferDisplayLayer`。两种都没有在本仓库验证过，需要先做一个小的可行性验证再定。
- **Windows 画面**【待验证】：往现有 `video_quad` 纹理写帧。该纹理固定 BGRA，所以要么在 CPU 上把 I420 转 BGRA 再 `UpdateSubresource`，要么改 vendored 渲染器加一个 YUV 采样通道。先做前者（不动 vendor 补丁），实测开销后再决定。
- **不用** GPUI 的 `surface()` / `img()`：前者在 Windows 是空操作，后者历史上有过逐帧上传导致内存只增不减的问题。

### 3.4 生命周期（两壳一致的语义）

| 事件        | 行为                                                                                 |
| ----------- | ------------------------------------------------------------------------------------ |
| 起播        | 建索引 → 从起点前最近关键帧解码 → 首帧到达即显示（不等时钟）→ 音频开始               |
| 暂停 / 恢复 | 暂停音频时钟；解码线程因帧队列满而自然阻塞                                           |
| seek        | 取消旧会话（原子标志 + 丢弃接收端）→ 新会话从目标前关键帧解起 → 丢弃目标之前的帧     |
| 倍速        | 时钟加速；解码侧无法跳帧（参考帧必须解），跟不上时丢显示帧，持续欠帧则进入缓冲       |
| 缓冲        | 欠帧时暂停音频，攒够预缓冲帧数后恢复                                                 |
| 切集 / 换档 | 取消旧会话，按新源重建；换档保留播放点                                               |
| 窗口关闭    | 取消会话，解码线程在下一次 send 或 read 处退出                                       |
| 错误        | 解码、读数、音轨错误都进入现有播放错误流程并带上原因；**不会**转去 H.264 或系统 HEVC |

线程与背压：每个会话一条专用解码线程；rusty_h265 单线程，vvdec 内部线程池不变。帧队列有界，满了就阻塞生产方。取消通过原子标志在“读数据前、每帧回调时”检查。不新增跨 `await` 持锁；解码在专用线程，不占 tokio 运行时。

### 3.5 支持边界

| 项                              | 计划                                                                                                                         | 依据                                          |
| ------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------- |
| 像素格式                        | 4:2:0（及 4:0:0）                                                                                                            | 现有帧契约是 I420                             |
| 位深                            | 8 / 10 bit 输入，输出统一 8-bit（截断右移，不抖动）                                                                          | rusty_h265 支持 Main / Main10                 |
| HDR                             | **不支持**（不做色调映射）。真实流是否有 HDR 未知                                                                            | 【待验证】                                    |
| 色彩空间                        | 现有路径不传色彩信息，交给浏览器默认。计划从 SPS VUI 读矩阵与范围并传给渲染端                                                | rusty_h265 是否暴露 VUI【待验证】             |
| 4:2:2 / 4:4:4、Range extensions | 不支持，明确报错                                                                                                             | 解码器会拒绝                                  |
| 流内分辨率变化                  | 线格式每帧带宽高，理论可行；未见真实样本                                                                                     | 【待验证】                                    |
| WebCodecs `VideoFrame`          | Windows WebView2 已有记录可用。WKWebView 需要较新的系统 WebKit，而安装包声明的最低系统是 macOS 10.15；Android WebView 未验证 | 【待验证】现有 ByteVC2 直播放有同样的隐含要求 |

---

## 4. 选流、能力探测与界面调整

### 4.1 选流

现状（已核实）：播放选流的能力是 `{hevc: 系统能解, bytevc2: 内置解码器可用}`；有 HEVC 时偏好 `h265/h264 > bytevc1 > bytevc2`；无 HEVC 时 `h264` 与 `bytevc2` 排最前；`codec_type` 为空按“能播”处理。

目标：

- 播放候选只保留内置解码器能处理的编码：HEVC 系（`h265 / h265_hvc1 / hevc / hvc1 / bytevc1`）和 ByteVC2。
- **H.264 不参与播放选流。** 某一档只有 H.264 时该档不可选；整集只有 H.264 时报明确的错误（新增文案，中英文同步）。
- **`codec_type` 缺失或未知不再按“能播”处理**，同样明确报错，并在日志里记下原始值。
- 同档位内的偏好建议保持 HEVC 系优先于 ByteVC2（HEVC 软解更省 CPU）。这一点请你确认。
- 下载选流不变（仍用 `DecoderCapability::full()`），不受本次改动影响。

风险：目前没有观测到服务返回 H.264，但如果某些剧或档位只有 H.264，这些内容会从“能播”变成“明确不能播”。这是你要求的行为，这里只是提醒它的影响面无法从现有数据估计。

### 4.2 能力探测与安装引导

- 播放不再读取 `hevc_playable()`（系统 HEVC 探测）。
- “安装 HEVC 扩展”的卡片（Tauri 的 `HevcInstallCard`、GPUI 的 `hevc_prompt`）在播放页失去意义，计划移除。
- 但 **Windows 上的兼容合并仍依赖系统 HEVC 解码**（平台转码链需要 HEVC MFT）。所以 `install_hevc_extension` 命令和系统 HEVC 探测本身要保留，供合并功能使用。合并页是否需要一个对应的提示入口，请你决定；我不会顺手加。

### 4.3 IPC、事件与错误

- 变更：`PlayResponse` 的直播放字段改名；协议子路径改名。
- 播放路径移除后不再使用的：`transcode_for_playback`、`cancel_compat_playback` 命令，`compat-play-progress` 事件，相关 zod schema 与前端订阅。按红线 3 多处同步删除。
- 新增错误文案：仅提供 H.264、编码未知、软解不支持的码流特性、软解失败。沿用 `AppError::Media` / `Unsupported`，不新增变体，除非实现时发现前端需要区分。

### 4.4 GPUI 可见界面改动

移除 `hevc_prompt`、把 ByteVC2“转码中”状态改成直播放的缓冲状态、新增错误文案展示，都属于可见界面改动。**在 `gpui-kit-design-guides` skill 可用并调用之前不动这些文件**，改完按规范跑 `make gpui-preview`。

---

## 5. 保留与可删除清单

**保留，不动：**

- 下载（取流、下载、解密、保存原始 MP4）。
- 快速合并（纯 remux）。
- 兼容合并及其依赖：`transcode_service::pipeline::transcode`、平台 H.264 编码链、`MergeMode::Compat`、设置页的转码后端显示与“重新检测”、`decode_capability` / `redetect_capability`、`install_hevc_extension`。
- HEIC 封面解码。
- 外部 ffmpeg 的显式调试开关（只影响合并与封面）。

**播放路径中可以删除的（确认后、功能验证通过后再删，单独提交）：**

- Tauri：`compat_play` 的播放入口、`use-transcode-fallback`、`use-growing-compat`、`growing-media`、`growing-fallback`、`compat-overlay`、`compat-policy` 的兼容分支、`hevc-install-card`、`hevc-session`、`use-media-recovery` 里转 H.264 的分支、`hongguo-protocol::grow`。
- GPUI：`open.rs` 的 ByteVC2 转码分支、`hevc_prompt`。
- 共用：兼容播放缓存（`.play.mp4`）的生成与统计。

**需要你决定的：**

- 离线 QA 本地播放入口（`qa_offline_local_play`）现在走 H.264 转码。改成走软解直播放，还是保留原样？
- `transcode_bytevc2`（vvdec 解码 + 平台 H.264 编码）：下载件不会是 ByteVC2，播放不再转码后，它只剩 QA 与示例在用。保留还是删除？

---

## 6. 分步实施计划

每一步都以“可编译、测试通过、可独立回退”为界；纯移动与行为修改分开提交。

### 阶段 0：证据（不改产品代码）

1. 取得真实样本：至少一集 ByteVC1（1080p 与 720p 各一）、一集 ByteVC2。最省事的来源是你用应用**下载**一集（下载件就是解密后的原始 MP4，且下载选流不会给 ByteVC2），ByteVC2 样本用仓库已有的 `#[ignore]` 探测测试生成。我不会自行调用在线接口去取。
2. 用样本确认 ByteVC1 的容器事实：fourcc（`hvc1` / `hev1`）、profile 与位深、是否有 B 帧、参数集位置、音轨格式。
3. 正确性对拍：同一份 ByteVC1 样本分别用 rusty_h265 和 ffmpeg / VideoToolbox 解出，逐帧比对（MD5 或 PSNR）。有任何差异就停下来分析。
4. 性能：在本机和至少一台低配 Windows 机器上测只解码的帧率（720p / 1080p）。Android 需要真机。
5. 根据 3、4 的结果确认或推翻路线 A。**推翻则暂停并更新本计划。**

### 阶段 1：抽出共用会话（纯结构移动）

把索引、seek、音轨遮罩、帧与平面池、读源、会话代次从 `hongguo-bytevc2` 和 `hongguo-tauri` 移到 `hongguo-softdec`，行为不变，ByteVC2 直播放照常工作。

### 阶段 2：接入 HEVC 解核

在 `hongguo-softdec` 里加 HEVC 分支：hvcC → 参数集、样本 → Annex-B、rusty_h265 解码、u16 平面 → I420、PTS 回带。用阶段 0 的样本做门控测试（没有样本时跳过，不伪造）。

### 阶段 3：Tauri 接通

协议与前端去 ByteVC2 化；`PlayResponse` 字段改名；HEVC / ByteVC1 走直播放。此时保留旧兜底代码，但播放不再进入。

### 阶段 4：选流与能力

按 §4.1 改选流；按 §4.2 调整能力读取；新增错误文案（中英文）。

### 阶段 5：GPUI

先做 macOS 与 Windows 各一个上屏可行性验证，再按 §3.3 实现；随后移除 ByteVC2 转码分支。界面改动受 §4.4 约束。

### 阶段 6：清理

按 §5 删除播放路径的 H.264 兼容代码，更新 `AGENTS.md`、`THIRD-PARTY-NOTICES.md`、`req-hg-003` 文档中与现状不符的描述。

### 阶段 7：Android

补 `android.yml` 的 vvdec 构建环境，触发 CI 构建；能否运行验证取决于是否有设备。

---

## 7. 验证计划

### 7.1 样本清单

| 样本 | 编码 / 分辨率 / 音频                             | 来源                  | 状态                           |
| ---- | ------------------------------------------------ | --------------------- | ------------------------------ |
| A    | ByteVC1 1080p，AAC                               | 应用下载件            | **缺**                         |
| B    | ByteVC1 720p，AAC                                | 同上                  | **缺**                         |
| C    | ByteVC2 720p，HE-AAC                             | 仓库 `#[ignore]` 探测 | **缺**（本机无）               |
| D    | 标准 HEVC 合成片段 1080p / 720p / 10-bit，无音轨 | 本机 x265 生成        | 已有，仅用于性能与无音轨边界   |
| E    | 仅 H.264 的集                                    | 未观测到              | **缺**，只能用构造的选流数据测 |

### 7.2 功能覆盖

在线与本地播放、冷启动首帧、暂停与恢复、前后 seek（含连续快速 seek）、连续切集、换档、1×–3× 倍速、无音轨（现有索引要求必须有 `mp4a`，需决定如何处理）、网络中断与恢复、播放到结尾、关窗后线程与内存释放。

选流边界：仅 H.264、编码缺失、编码未知、同集不同档编码不同、解码失败。这些用确定性单测覆盖，数据标明是构造的。

### 7.3 “确实没走旧路径”的可观察证据

- 日志：播放期间不出现系统 HEVC 探测、VideoToolbox / MF 解码会话、H.264 编码会话、ffmpeg 进程的日志行；出现软解会话的起止日志。
- 状态：播放后 `compat-cache/` 目录无新增文件。
- 进程：播放期间没有 ffmpeg 子进程。
- 测试：源码静态测试断言播放模块不再引用 `transcode_service` 与 `compat_play`（仓库已有这种“扫源码钉约定”的惯例）。

### 7.4 性能指标

起播耗时、CPU 占用、内存峰值、显示帧率与丢帧数、seek 到出画的耗时、音画偏差。每条记录写明设备、系统版本、样本。Tauri 侧可复用 `scripts/bytevc2-playback-audit.mjs`（需改名与去 ByteVC2 化）。

### 7.5 平台矩阵（如实记录，不把编译通过写成播放通过）

| 平台                       | 构建                              | 运行验证                           |
| -------------------------- | --------------------------------- | ---------------------------------- |
| macOS arm64（Tauri、GPUI） | 本机可做                          | 本机可做                           |
| macOS x86_64               | 本机可交叉编译                    | **无设备，无法验证**               |
| Windows x64（Tauri、GPUI） | 只能靠 CI                         | **本机无法验证**，需要你或 CI 环境 |
| Android arm64（Tauri）     | 只能靠 CI，且当前工作流大概率不通 | **无法验证**，需要真机             |

### 7.6 命令

- Rust：`cargo test --workspace --lib -- --test-threads=1`、`cargo clippy --workspace --all-targets -- -D warnings -D clippy::allow_attributes`、`cargo fmt --check`。
- 前端：`pnpm typecheck`、`pnpm test`、`pnpm lint`、`pnpm knip`。
- 收尾：`make lint`、`pnpm build`；GPUI 界面改动后 `make gpui-preview`。
- 门禁命令裸跑并确认退出码。
- 本机构建注意：仓库路径含空格时 vvdec 构建会失败，需把 `CARGO_TARGET_DIR` 指到无空格路径，并设 `SYSTEM_DEPS_LIBVVDEC_BUILD_INTERNAL=auto`。

---

## 8. 风险

| 风险                                  | 影响                      | 应对                                   |
| ------------------------------------- | ------------------------- | -------------------------------------- |
| ByteVC1 含非标准特性，软解器解不对    | 路线 A 不成立             | 阶段 0 对拍；失败则暂停                |
| rusty_h265 单线程在低端设备达不到实时 | 卡顿，高倍速不可用        | 阶段 0 实测；按 §2.4 处理              |
| rusty_h265 版本新、接口不稳           | 升级成本、潜在缺陷        | 钉版本；对拍测试常驻                   |
| GPUI 原始帧上屏在两端都没有现成路径   | 工期与不确定性最大        | 先做可行性验证再承诺                   |
| 部分内容只有 H.264                    | 这些内容变为不可播        | 明确报错；上线前用真实数据评估比例     |
| 旧系统 WebView 缺 WebCodecs           | Tauri 在旧 macOS 上无画面 | 起播前检测并明确报错；评估最低系统版本 |
| Android 构建链本身未验证              | 无法交付 Android          | 单列阶段 7，不与桌面绑定               |
| 色彩矩阵 / 范围不对                   | 偏色                      | 读 VUI 并传给渲染端；用样本目检        |
| HEVC 专利                             | 分发风险                  | 需你或法务确认                         |

---

## 9. 需要你确认的事项

1. 是否安装 `dev-habits` skill（会从第三方仓库下载）；`gpui-kit-design-guides` 不可用时 GPUI 界面步骤如何处理。
2. 解码器：是否同意路线 A（扩大已有依赖 rusty_h265 的用途，钉 `=0.7.1`）；路线 B 仅在阶段 0 推翻 A 时再议。
3. 是否同意新增 workspace crate `hongguo-softdec`，以及 §3.1 的代码移动。
4. 性能不达标时的原则（§2.4）。
5. 同档位内 HEVC 系优先于 ByteVC2 是否符合预期（§4.1）。
6. 离线 QA 入口与 `transcode_bytevc2` 的去留（§5）。
7. 合并页是否需要 HEVC 扩展提示（§4.2）。
8. 无音轨视频如何处理（§7.2）。
9. 真实样本由谁、以什么方式提供（阶段 0）。
10. Windows 与 Android 的验证由谁执行（§7.5）。
11. HEVC 专利方面是否需要先做核查（§2.5）。
