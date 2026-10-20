# GPUI 系统播放器：进度与交接（macOS 已完成，Windows 已完成 Phase 2）

最后更新：2026-10-20

## 1. 已完成并验证

| 项 | 结果 | 提交 |
| --- | --- | --- |
| Phase 0：AVPlayer 可行性 | 100Hz 下 1x/2x/3x 零丢帧；seek 0 帧误差；切集 60ms | `ce25dbc` |
| Phase 1a：渐进 Range 供数 | 真实在线 70–80ms 切集；2x 60fps；seek 0 误差 | `b036c2e` |
| Phase 1b：接入窗口形态 | 常态/小窗/竖屏/铺满/全屏，切形态不断播；2x 60fps 零合并 | `ca14637` |
| 透明洞修复 + 默认 system | 画面可见；2x 48fps（内容 24fps×2）满帧、帧间隔 20.2ms、0 丢帧 | `ed068c7` |
| 已看完跳过 + 续播 clamp + 加载条 | 已看完不续播/不预取；剩余 <2s 直接下一集；加载条首帧后灭 | `9e7fab5` |
| 冷切集 + 8MB Range 超时 | 冷切中位 37ms；75s/2x 无 8MB 超时 | `9e7fab5` |
| 片尾单一真相源（本轮） | 删除 30fps 尾部兜底，只认 Ended + 三代次幂等；长跑待复验 | `5e127fc` |
| **Phase 2 Windows（本轮）** | MediaEngine + 自定义 `IMFByteStream` + DComp 视频位；真实壳 2x 60fps、0 丢帧、切集 287ms | 本次提交 |

## 2. 已验证（本机）

- 系统后端五种窗口形态 30fps 稳定、切形态不中断；
- 2x 满帧、零丢帧、帧间隔 ~20ms、音画由 AVPlayer 保证；
- seek 精确（0 帧误差）、切集热路径 15–53ms；
- 已看完的剧：历史页/详情页点击直接跳下一部，不起播放器、不预取、加载条不亮；
- 续播剩余 <2s：不起播放器，直接下一集 0s。

## 3. 未完成

| 项 | 说明 | 优先级 |
| --- | --- | --- |
| ~~Phase 2 Windows~~ | **已完成**：MediaEngine + 自定义 `IMFByteStream` + DComp 视频位，见 [`gpui-system-player-windows.md`](./gpui-system-player-windows.md) | P0 |
| Windows 切集提速 | Windows 无播放列表，切集走重建（实测 287ms）。方案：预热一个「离屏渲染的下一集引擎」，转正时把视频位的 `SetContent` 换到它的视觉上再 Play（≈一帧）；vendored 补丁需加 `offscreen_visual` / `set_content` 两个小接口 | P1 |
| 首播 0.7–3.7s | 取流表 + 渐进探测慢，可并行/预注册 | P1 |
| 遮挡暂停 | 窗口完全被遮挡时暂停（现在只有最小化/老板键） | P1 |
| ~~音频客观指标~~ | **已关闭**：系统播放器直接出声（平台音频栈），实测 2x/3x 无爆音与欠载听感差异；需要客观读数时用系统级工具，不在壳里做 audio tap | — |
| 音高声学复验 | `Spectral` 仅配置级，2x 听感未实测（系统后端无此配置，属自研引擎项） | P2 |
| 预加载句柄上限 | `LoaderHandle` 按集累积，未做两槽上限 | P2 |
| loader 池生命周期 | 4 worker 进程级，退出不 join | P2 |
| 多窗口/多显示器 | 系统播放器为单窗口全局宿主 | P2 |
| 预览截图 | `render_to_image` 抓不到原生视频层（macOS AVPlayerLayer / Windows DComp），视频区黑，需系统截图 | P2 |

## 4. 待验证（重要）

1. **20 次自然连播无跳集**：单集 60–180s，未跑满；已用“接近片尾起播 + Ended 一次”两条路径验证单次幂等，需长跑复验。
2. **重复 Ended / 跨 item 旧回调**：代次能拦，但极端时序需长跑观察。
3. **历史缺 `episode_cnt`**：`series_finished` 无法判定，可能仍起播（有节拍兜底，但会多一次起播）。
4. **弱网下切集**：接近 200ms 目标时是否需要双 player + 双 layer 预热换层。
5. **回滚路径**：`VIDEO_BACKEND=gpui` 已烟测，未做完整回归。

## 5. 运行方式

```bash
make gpui                      # macOS 默认 system（AVPlayer）；Windows 默认 system（MediaEngine）
VIDEO_BACKEND=gpui make gpui   # 回滚 GPUI 自研上屏
VIDEO_BACKEND=system make gpui # 显式系统后端

HONGGUO_AV_METRICS=1           # 5s 一次帧率/间隔统计（macOS）
HONGGUO_WM_METRICS=1           # 5s 一次 rendered/dropped 统计（Windows）
HONGGUO_AV_RATE=2              # 首帧后切 2x
HONGGUO_AV_NEXT_EPISODE_MS=5000 # 首帧后 5s 切下一集
```

Windows 下抓帧取证：DComp 内容不进窗口重定向表面，`PrintWindow` 抓到的是
黑图，只能用屏幕区域拷贝（窗口提前后 `BitBlt` 窗口矩形）。

## 6. Phase 2 Windows（已落地）

- 设计/实测/坑：`docs/gpui-system-player-windows.md`
- 平台开关：`crates/hongguo-gpui/src/video_backend.rs`（macOS/Windows 默认 system）
- Windows 实现：`crates/hongguo-gpui/src/wm_presenter/`（mf/stream/events/view/player）
- 视频位挂点：`vendor/gpui-pre-windows` 的 `system_video` 模块（补丁说明见根 `Cargo.toml`）
- 引擎创建失败：`video_backend::fall_back_to_gpui()` 翻开关 + 重走起播，不 panic

## 7. 风险

- 已看完判定依赖历史 `episode_cnt`；
- `Ended` 幂等依赖每 item 单次回调，长跑待验；
- 系统后端无逐帧回调，弹幕/进度靠 30fps 节拍；
- 音频由系统引擎输出（macOS AVPlayer / Windows MediaEngine），无欠载指标；
- Windows 的视频位是 vendored gpui 补丁，上游升级时要重新对一遍视觉树改动。
