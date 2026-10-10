# 登录通道评估：设备注册 / 抖音授权 / 扫码登录（2026-10-10）

依据：官方 APK（com.phoenix.read 7.3.9.32）静态提取 + **jadx 反编译源码**（classes16/20）
+ 本项目实现走读。APK 分析产物：`C:/Users/liu13/tools/rea/analysis/hongguo-apk/`。

## 1. 设备注册：✅ 已对齐，维持现状

项目 `register.rs`（`domain/api/register/mod.rs`）从 hgplayer 抓包模板构建：

- query 40 项：**37 项在官方 dex 字符串表逐字在场**；3 项（`is_guest_mode`/
  `is_preinstall`/`last_deeplink_update_version_code`）官方运行时拼参，项目侧有
  抓包原样背书，无碍。
- body header 47 项指纹字段：**官方 dex 全数在场**；`magic_tag=ss_app_log` +
  `_gen_time` 结构一致。
- 加密：`tt_encrypt_v5` 与官方 `libEncryptor.so` 同族（官方 RegisterNatives 无 Java
  导出，我们用抓包还原算法）。
- 官方宿主是 AppLog SDK 启动流程（`com.bytedance.applog.*`，含 retry/throttle/
  oaid_refine 生命周期）；本项目独立单发——**wire 层对齐**，触发时机差异无验证
  手段且无必要。

结论：**不改**。维持「已实现、未接入主流程」状态。

## 2. 抖音授权登录（拉起抖音 App 版）：❌ 桌面不可行

官方链路是字节 OpenSDK **app-to-app** 授权：`com.bytedance.sdk.open.aweme.authorize.*`
+ manifest 声明查询抖音包（`com.ss.android.ugc.aweme`），要求**同一台设备装有抖音
App**，deeplink 拉起授权、回调落 `BdAuthorizeActivity`。桌面没有可 IPC 的抖音 App，
这条链路不可复现。**不做。**

但「抖音扫码登录」走的是另一条**纯 HTTP 协议**（见 §3）——桌面完全可复现。

## 3. 扫码登录（抖音扫码授权）：✅ 协议已从官方 APK 反编译拿全，可直接实现

来源：`com.bytedance.sdk.open.tt.j`（QRCodeAuthTask，classes20）+ `ch1.k`
（get_oauth_token）+ `ub4.j0`（登录通道配置，classes16）。**无需抓包即可实现**——
这是抖音开放平台的标准 PC 扫码授权协议（`source=pc_auth`）。

### 3.1 协议全貌

**① 取码** `GET https://open.douyin.com/oauth/get_qrcode/`

| 参数 | 值（官方反编译实测） |
| --- | --- |
| `aid` | `1128`（抖音开放平台固定值） |
| `client_key` | **`awhrxjuqewhhyckk`**（红果的抖音 client_key，`ub4.j0` 硬编码） |
| `scope` | 请求的权限串 + verifyScope（空格去除） |
| `state` | 透传 |
| `next` | redirect_uri（透传） |
| `jump_type` | `native` |
| `optional_scope_check` / `optional_scope_uncheck` | 可选授权项 |
| `customize_params` | `{"comment_id":…,"source":"pc_auth","not_skip_confirm":"true"}` |
| `source_from` | `native` |
| `device_platform` | `android` |
| `app_identity` | `Md5Utils.hexDigest(...)`（一次性缓存；输入见 SignatureUtils） |
| `signature` | `SignatureUtils.packageSignature(getMd5Signs(ctx, 包名))`——基于 APK 签名 MD5（官方 sig_hash `aea615ab…`，register.rs 已有同值）派生 |

→ 响应 `data.qrcode` = **base64 二维码图片**（直接展示，前端不用自己画码）、
`data.token` = 轮询令牌。

**② 轮询** `GET https://open.douyin.com/oauth/check_qrcode/`（同上参数 + `token` +
`timestamp=<ms>`；间隔 1000ms，服务端配置 `qrcode_auth_config.polling_interval`）

状态机（`l.c` 字段）：`new`（继续轮询）→ `scanned`（通知 UI + 继续轮询）→
`confirmed`（`data` 带 `auth_code` + `granted_permissions`）｜`refused`（errorCode=-2
用户取消）｜`expired`（-50 二维码过期）。失败重试计数递减。

**③ 换票** `POST /passport/auth/get_oauth_token/`（passport 域，参数 form）
`platform_app_id=7828`（番茄系第三方平台 id，SDK 硬编码）+ code/state 等映射；
另有 `/passport/auth/get_oauth_token/v2/` + `access_token` 变体。
→ **红果 passport 会话**（Set-Cookie 全家桶 + token，复用 `extract_cookie_pairs`
落库口径）。

### 3.2 待实现时逐项核实的（反编译已定位源文件）

- `app_identity`/`signature` 的精确输入串：`com.bytedance.sdk.open.aweme.utils.
  Md5Utils / SignatureUtils`（已反编译在 jadx-src20，实现时照抄公式）；
- `get_oauth_token` 的 map 键名（code/state）：`DouyinAuthHelper` 调用链
  （jadx-src20，已反编译）；
- scope 取值：看官方实际登录用的 scope 串（`Authorization.Request` 构造处）。

### 3.3 与 passport 扫码（另一条通道）的关系

`/passport/open/scan_qrcode/` + `/passport/mobile/confirm_qrcode/` 是**手机确认侧**
（创作者中心扫码授权，`CreatorCenterQrCodeScanAuthActivity`，body
`{token, scene, qr_source_aid}`）——与桌面扫码登录方向相反，**本项目不用**。

## 3.4 运行时实测结论（2026-10-10，桌面构建实调）

协议实现已提交（feature/douyin-qrcode-login 分支），真机直调两种端点均被
**服务端权限墙**拒绝，客户端无法绕过：

| 端点 | 实测结果 |
| --- | --- |
| `open.douyin.com/oauth/get_qrcode/` | `error_code=10015 应用类型错误`——红果的 client_key 是 **APP 类型**，该端点仅服务**网页类型**应用；补官方 UA/Origin 等头无效，换 UA/Origin/Referer 组合均 10015 |
| `novel.snssdk.com/passport/web/qrcode/generate/` | `error_code=16 该应用无权限`——aid=8662 无 web 扫码权限 |
| 同端点换番茄 aid=1967（阳性对照） | 同样 `error_code=16`——番茄域也未开放，证明非参数问题，是该账号域整体未开 web 扫码 |

结论修正：**扫码登录当前对第三方客户端不可行**（两条路都是官方服务端配置
未开放，反编译协议再完整也过不了权限校验）。协议实现保留在分支上——若官方
日后开放（APP 类型 key 开放 QR 接口 / aid 开 web 扫码权限），翻转即用。
扫码通道评估到此**关闭**；登录通道以短信登录为唯一已落地路径。

## 4. 汇总

| 通道 | 结论 | 动作 |
| --- | --- | --- |
| 设备注册 | 已对齐（抓包背书 + 官方 dex 佐证） | 维持现状 |
| 抖音授权（拉起 App） | 桌面不可行（app-to-app） | 不做 |
| **抖音扫码登录** | **协议反编译拿全，client_key 实锤** | **P2 可直接实现** |
| passport 手机确认扫码 | 方向相反（手机确认侧） | 不用于桌面 |
