# 红果官方 App API 端点清单（2026-10-03 抓包）

来源：hgplayer v1.1.2（第三方客户端，直连官方 API）经 mitmproxy 抓包实测，
全部端点均在 `api5-normal-lq.fqnovel.com`，签名五件套
（x-gorgon/x-argus/x-ladon/x-helios/x-medusa）+ `x-reading-request: {ticket_ms}-{random}` 头。
响应普遍为 **brotli 压缩 JSON**（请求头带 `accept-encoding: gzip, deflate, br`）。
业务参数一律放 **URL query**（POST 也是），POST body 另有 JSON 业务体。

公共参数（每个请求都带，签名覆盖）：`device_platform=android, aid=**, version_code=73932,
manifest_version_code=73932, app_name=**, channel=**, device_type=Xiaomi 14 系指纹,
device_id/iid/openudid/cdid, os=android, os_version, dpi=460, resolution,
host_abi=arm64-v8a, _rticket={ms}, sim_region, msToken...`

**请求形态两族**（对齐时别混）：

| | lq 域 reading 系 | sinfonlineb 域播放/推荐流 |
|---|---|---|
| 端点 | 榜单/新剧/日历/预约列表/搜索/弹幕/预约操作 | multi_video_model / landpage |
| 头 | x-ss-dp + lc + x-reading-request（无 gorgon/argus） | 全签名五件套 |
| POST body | **一律 gzip**（`Content-Encoding: gzip`） | 项目自有验证形态（未压缩，服务端两种都收） |

⚠️ **参数必须逐值对齐抓包，不能凭"也能用"保留旧值**——教训：
send_code 的 `type=1` 能发码但被服务端按"换绑"场景处理（目标号已绑定其它
账号时报 1001），`type=3731` 才是短信登录发码场景（2026-10-04 实测）。

## 1. 排行榜（内容 tab × 子榜 × 筛选面板）

```
GET /reading/bookapi/bookmall/cell/change/v        # 注意 /v 不带斜杠
```

固定参数：`cell_id=7470092475068071998, tab_type=26, selected_items=all,
category_id=0, cell_sub_id=0, client_req_type=2, client_template=2, gender=2,
limit=0, offset=0, unlimited_selector_change_type=2`

### 1.1 内容 tab 与子榜（2026-10-05 抓 hgplayer 1.1.3 锁定）

顶部内容 tab 用 `selected_items` 切换，子榜用 `sub_selected_items`，
两者成对出现：

| 内容 tab | selected_items        | 子榜 sub_selected_items（部分） |
|---------|----------------------|--------------------------------|
| 全部     | `all`                | `ranklist_hot_sc` 等 8 个（下表） |
| 真人剧   | `human`              | `human_hot_sc` / `human_hot_play` / `human_new_rank` / `human_hot_search` / `human_must_watch` / `human_followed` |
| 漫剧     | `comic_series_rank`  | `comic_series_hot_rank` / `comic_series_hot_play` / `comic_series_new_rank` / `comic_series_hot_search` |
| AI剧     | `ai_playlet`         | `ai_playlet_hot_sc` 等 6 个 |
| 演员     | `ranklist_celebrity` | 无子榜；**响应 video_data=0**（条目是演员形态），客户端剧集 UI 跳过 |
| 系列剧   | `series_album`       | `series_album_hot_sc` / `series_album_new_rank`（新季榜） |

「全部」tab 的 8 个子榜：

| 榜单   | sub_selected_items       |
|------|--------------------------|
| 推荐榜 | `ranklist_hot_sc`        |
| 热播榜 | `ranklist_hot_play_sc`   |
| 臻果榜 | `ranklist_prestige`      |
| 预约榜 | `ranklist_subscribe`     |
| 新剧榜 | `ranklist_new_rank_sc`   |
| 热搜榜 | `ranklist_hot_search_sc` |
| 必看榜 | `ranklist_must_watch`    |
| 收藏榜 | `ranklist_followed`      |

### 1.2 筛选面板（panel_selected_items）

面板选中项用 `panel_selected_items` 传值，**单值**——hgplayer 抓包实测
每次点击整组替换（女频→点古装后只剩 `cate_308`，不带 `gender_female`）：

- 行「综合」：总榜（不传参）/ `gender_female` 女频 / `gender_male` 男频
- 行「时代背景 / 主题情节 / 角色设定」：`cate_*`（古装=cate_308、
  逆袭=cate_739、萌宝=cate_28…）
- 漫剧子榜多一行「画风」：`style_*`（3d=style_1685…）

**选项表由响应随行下发**：`data.cell_view.cell_selector.outer_row.items[]`
（内容 tab）→ `sub_cell_selector.outer_row.items[]`（子榜）→
`panel_selector.inner_rows[]`（筛选行，`row_name` 行名、`selection_type=1`
单选、`items[].selector_item_id`，空 id = 总榜）。完整样本
`captures/rank-selector-schema.json`。我们的实现（`rank.rs
parse_cell_selector`）原样展开给前端渲染，不在前端硬编码选项。

响应：`data.cell_view.cell_data[]`（每块=1 条，`cell_name="小卡"`、`show_type=505`），
条目在 `video_data[]`：

- `title` 书名、`series_id`、`vid`、`cover`（HEIC，需转官网 webp 方案）
- `sub_title`（"玄幻·全200集"）、`sub_title_list[]`（分类标签）
- `score`（"8.0"）、`play_cnt`、`episode_cnt`、`duration`
- `rec_text_item.RecommendText`（"13707万最高热度"）
- `secondary_info_list[]`（"258.7万收藏"）
- `recommend_info`（JSON 字符串，含 `rank` 排名）
- `video_desc` 简介、`category_schema` 分类 schema JSON

## 2. 新剧推荐

```
GET /reading/bookapi/bookmall/cell/change/v1/      # 注意 v1/ 带斜杠，与排行榜不同
```

参数：`cell_id=7431550523368554558, selected_items=firstonlinetime_new,
cell_gender=2(全部)|1|0, change_type=1, client_req_type=1, limit=18, offset=0`

响应结构与排行榜相同（cell_data[].video_data[]）。

## 3. 上新日历（新剧页第二 tab）

```
GET /reading/user/subscribe/list/v1/
```

参数：`active_panel=6, gender_type=2, need_calendar_schema=true, tab_style=2, tab_type=5`

**切日期：`target_date=YYYYMMDD`**（2026-10-04 抓 hgplayer 切日期锁定；此前试过的
`date` 等 8 个候选名全部无效）。传它服务端只回该日条目，另附
`calendar_loc_info{start_pos,end_pos,down_has_more,up_has_more}`。

响应条目有**两种 schema**，服务端按请求形态分发：

| 形态 | 触发条件 | 条目结构 |
|------|---------|---------|
| 嵌套 | 匿名请求（无 install_id Cookie） | `subscribe_data.{series_id,title,vid,score,...}` + 顶层 `category/rec_tags/schedule_publish_time/is_online` |
| 扁平 | 带 install_id Cookie 或 target_date | 字段直接在条目上：`item_id,name,cover,item_desc,sub_title_list[],category,categories,schedule_publish_time,expected_publish_time,is_online`；**无 vid**（点击走 seriesId 解析详情） |

首页（不带 target_date）返回默认日（今天）条目 + `calendar_schema.date_list`
（前后各一周）+ `default_date`；条目跨日分布时 `has_more/next_offset` 翻页。

## 4. 找剧（筛选浏览）

```
POST /reading/distribution/category/landpage/v1/   # 与首页推荐同端点，JSON body
```

```json
{"client_req_type":3,"filter_ids":"","limit":18,"need_selector_panel":false,
 "offset":0,"req_scene":"default","req_type":"only_content",
 "select_items":{"category_dim_epoch":[],"category_dim_role":[],
   "category_dim_theme":[],"gender":[],"genre":[],"online_time":[],"sort":[]},
 "session_id":"20261003165900B053FA3876D73D9E20A0"}
```

筛选维度：`genre`(体裁) `category_dim_theme`(主题) `category_dim_role`(设定)
`category_dim_epoch`(背景) `sort`(推荐/最新上架/最高热度/最高收藏)
`gender`(受众 男频/女频) `online_time`(7/14/30/90 天内上新)。

## 5. 搜索

```
GET /reading/bookapi/search/tab/v
```

参数：`query=丧尸, tab_type=11, count=100, offset=0, use_correct=true,
bookshelf_search_plan=4, need_personal_recommend=1`

翻页：`offset=N&passback=N&search_id={首响应返回的 search_id}`（实测综合 tab
首页只回 4~6 条，next_offset 顺延，第二页起 ~100 条/页）

**风控要求（2026-10-04 实测）**：必须带有效 `install_id`（query 的 `iid` 与
Cookie 的 `install_id` 一致 + `ttreq` 票）。失效 id 的表现是 **HTTP 200 + 0 字节**
静默拒，且会连坐其他 reading 接口。请求头无 x-gorgon/x-argus/x-ladon
（reading 轻签名：`x-ss-dp + x-reading-request + lc` 即可，多带也能过）。

响应：`search_tabs[]`（tab_type: 综合=11 / 剧集=29 / 视频=30 / 讨论=31 /
小说=1 / 用户=27 / 听书=2）。综合 tab 的 `data[]` 每条含 `video_data[]` 剧集数据，
`has_more`/`next_offset` 控制翻页。

## 6. 我的预约

```
GET /reading/user/subscribe/list/v1/
```

参数：`is_online=true(已上线)|false(待上线), limit=20, offset=0,
subscribe_offset=0, subscribe_order_type=0, swipe_type=0, tab_type=13`

登录后条目为扁平形态（`item_id/name/has_subscribed/schedule_publish_time/...`），
`data.online_total_count` / `offline_total_count` 是两栏计数。

## 7. 预约 / 取消预约（2026-10-04 抓 hgplayer 1.1.3 实操锁定）

```
POST /reading/bookapi/search/uncover_subscribe/v
```

- **body 是 gzip 压缩的 JSON**（头带 `Content-Encoding: gzip`）：
  `{"item_id": <series_id>, "item_type": 1, "op_type": 1 预约 / 2 取消,
  "shark_param": {埋点上下文，不校验}, "wish_list_all_del": 0}`
- query 只放设备指纹；reading 轻签名头（x-ss-dp/lc/x-reading-request）；
  **必须带登录 cookie**，响应 `code==0` 即成功
- ⚠️ hgplayer 的预约按钮由 **WebView 前端 fetch** 发出——Windows 上
  Chromium 走**系统代理**而非进程 env 代理，抓它需临时把系统代理指向
  mitmproxy（操作完记得还原用户的原代理）

## 8. 短信登录（passport 系，1.1.3 真机实证）

```
POST /passport/mobile/send_code/v1/   mobile 密文 + type
POST /passport/mobile/sms_login/      mobile/code 密文 + csrf cookie
POST /passport/upsms/verify/          MFA 上行短信轮询（form body）
```

- `mobile` / `code` 都是 **XOR(0x05) hex 密文**；mobile 密文须带
  **`+86` 国码前缀**（不带按残缺号处理）；`mix_mode=1`
- 发码响应 **Set-Cookie `passport_csrf_token`** = 会话绑定凭据，
  sms_login 的 cookie 带上它（body **不传** mobile_ticket——1.1.3 实测）
- `type`：发码场景参数，1 与 3731（1.1.3 实测值）都受理
- 登录成功 Set-Cookie 下发会话全家桶：`sessionid`/`sid_tt`/`sid_guard`
  （60 天）/`uid_tt`/`odin_tt`/`d_ticket`/`passport_mfa_token` 等
- 参数放 query 还是 body form-urlencoded 均可（服务端都解析）；
  发码对格式合法的号不做真实性校验（179 虚构号也发码成功）
- 错码响应 `error_code=1202`（"验证码错误"，data.description 带文案）

**MFA 上行短信流程**（换设备/风险登录触发，error_code=2046）：

1. sms_login 返回 `data.biz_params.{passport_mfa_retry_tag, sms_code_key}`
   + `encrypt_uid` + `event_params{log_id,verify_reason,verify_scene}` +
   `common_params{copywriting_key,ies_safety_diversion_tag}` +
   `verify_ways[]`（`mobile_up_sms_verify` 项含 `channel_mobile` 通道号与
   `sms_content` 回复内容，如 回复 "YZ"）
   **通道号陷阱（2026-10-05 实测）**：API 下发 `channel_mobile=9515211003`
   （宁夏银川 95 扩展号段）——**回复到它服务端收不到**。hgplayer 1.1.3 把
   `9515211003` 与 `10691859839103` 两个号码都硬编码在二进制里做替换，
   真实可回复通道是 `10691859839103`（运营商 106 网关；抓包全量数据中
   不存在该号码，只能来自客户端内置）。我们已在 `login.rs
   real_upsms_channel` 做同款替换，未知号码透传（换号靠抓包发现）。
2. 轮询 `POST /passport/upsms/verify/`：body 为 form（`biz_params` 是
   JSON 串 + 上述上下文字段 + 常量 `new_authn_sdk_version=1.1.31`、
   `request_tag_from=h5`，另有两个空值字段 `new_verify_flow=`、
   `verify_ticket=`）；`error_code=1045` = 等待用户回复短信。
   **MFA 会话绑定（2026-10-05 真机踩坑）**：触发 MFA 的 sms_login 响应
   会 Set-Cookie `passport_mfa_token=<短token>`——轮询请求的 Cookie
   **必须带它**，缺了服务端永远回 1045（回复短信也没用）
3. 用户回复短信后轮询返回 `data.registered=true`（附 ticket，可忽略），
   **该响应再 Set-Cookie 一个新的长 `passport_mfa_token`**——重登请求
   必须带这个新值。**此时还没有会话**——再发一次 sms_login（原 code
   密文 + `passport_mfa_retry_tag` 明文 + `sms_code_key` **密文**）成功
   拿 cookie
4. 退出登录**无网络请求**（hgplayer 纯本地清账号）

## 会话有效期与 x-tt-token（2026-10-05 逆向 hgplayer + 落库数据）

- **sessionid 有效期 60 天**：`sid_guard = <sessionid>|<登录时间戳>|5184000|<Expires>`
  （5184000s = 60 天）。手机"登录一次在线很久"的直接原因。
- **续约**：hgplayer 二进制无任何 refresh/auth 端点（passport 路径只有
  send_code/sms_login/upsms）——token 里的 refresh 段不被主动使用，
  最可能是服务端对活跃用户在响应头滑动续期（抓包 2h 窗口内凭据无变化，
  未实证；将来可在 addon 记录响应头验证）。过期后重新短信登录。
- **x-tt-token 双 token**：登录**响应头**下发 `00<access 195位>--<refresh
  150位>-3.0.3`（Set-Cookie / body 里都没有）；请求带**前 56 位短形式**
  （`00`+sessionid+22 位），hgplayer 每个请求都在场。我们已对齐：
  `AccountState.token` 落库 → `ApiEnv.x_tt_token` → `send_once` 注入头。
  旧账号该字段为空（不带），下次登录自动补上。
- 登录态完整凭据：sessionid / sid_tt / sessionid_ss（同值）+ sid_guard +
  uid_tt(_ss) + odin_tt + d_ticket + n_mh + session_tlb_tag 等 17 个 cookie
  （`extract_cookie_pairs` 全收，同名后值覆盖）+ x-tt-token 头凭据。

## 已知未抓 / 待做


- 设备注册 `POST log.snssdk.com/service/2/device_register/`（旧会话已抓到
  query 全指纹 + protobuf body + gzip 响应，尚未实现；bookmall 全家桶在新设备上
  才不报 ILLEGAL_ACCESS 110）
- 历史/收藏/点赞页未抓（疑似本地存储或需登录，优先级低）
- 登录（短信+扫码）最后做
- 弹幕发送 `commentapi/comment/add` 未实现（拉取 `comment/list` 已落地）

## 抓包数据文件

历史轮次：`C:\Users\liu13\AppData\Local\Temp\hg_capture\`（易失，随时可能被清）

**现行工作流（自持抓包）**：仓库 `captures/`（已 gitignore，持久保留）——

- `audit.py` **参数对齐审计（硬规矩：新接口/改接口必须先跑它逐字段对齐，
  不许凭"大概有用"挑字段——2026-10-05 前 type=3731 / channel_mobile 替换 /
  passport_mfa_token / new_verify_flow 空字段连续四次丢参后的整顿产物）**：
  `python captures/audit.py [接口名过滤词]` 输出每个接口抓包在场的
  query/form body 字段/cookie 键/关键头。注意 addon 存的 cookie 是逗号
  分隔（audit 已兼容）。`d_ticket` 是服务端历史下发凭据（我们没有
  来源），不算丢参。

- `addon.py` mitmproxy dump 脚本（目标域名过滤，输出 JSONL，MITM_OUT 可覆盖输出路径）
- `flows-YYYYMMDD.jsonl` 按日分轮的抓包产物（req 头/参数/resp body b64+brotli）

重放工作流：`mitmdump.exe -p 8080 -s captures/addon.py`（mitmproxy 12.2.3 在
`%LocalAppData%\Programs\Python\Python314\Scripts\`，CA 已在
`~/.mitmproxy\` 且系统已信任）→ 以 `HTTP_PROXY/HTTPS_PROXY=http://127.0.0.1:8080`
环境变量启动 `C:\Users\liu13\Downloads\hongguo-v1.1.2-windows-amd64.exe`
（Wails/Go 后端吃 env 代理）→ computer-use 驱动 UI 触发目标接口。
