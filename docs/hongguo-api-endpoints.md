# 红果官方 App API 端点清单（2026-10-03 起持续抓包）

来源：hgplayer（第三方客户端，直连官方 API）经 mitmproxy 抓包实测
（v1.1.2 起，现行 1.1.5），
全部端点均在 `api5-normal-lq.fqnovel.com`，签名五件套
（x-gorgon/x-argus/x-ladon/x-helios/x-medusa）+ `x-reading-request: {ticket_ms}-{random}` 头。
响应普遍为 **brotli 压缩 JSON**（请求头带 `accept-encoding: gzip, deflate, br`）。
业务参数一律放 **URL query**（POST 也是），POST body 另有 JSON 业务体。

公共参数（每个请求都带，签名覆盖）：`device_platform=android, aid=**, version_code=73932,
manifest_version_code=73932, app_name=**, channel=**, device_type=Xiaomi 14 系指纹,
device_id/iid/openudid/cdid, os=android, os_version, dpi=460, resolution,
host_abi=arm64-v8a, _rticket={ms}, sim_region, msToken...`

**请求形态两族**（对齐时别混）：

|           | lq 域 reading 系                                    | sinfonlineb 域播放/推荐流                  |
| --------- | --------------------------------------------------- | ------------------------------------------ |
| 端点      | 榜单/新剧/日历/预约列表/搜索/弹幕/预约操作          | multi_video_model / landpage               |
| 头        | x-ss-dp + lc + x-reading-request（无 gorgon/argus） | 全签名五件套                               |
| POST body | **一律 gzip**（`Content-Encoding: gzip`）           | 项目自有验证形态（未压缩，服务端两种都收） |

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

| 内容 tab | selected_items       | 子榜 sub_selected_items（部分）                                                                                   |
| -------- | -------------------- | ----------------------------------------------------------------------------------------------------------------- |
| 全部     | `all`                | `ranklist_hot_sc` 等 8 个（下表）                                                                                 |
| 真人剧   | `human`              | `human_hot_sc` / `human_hot_play` / `human_new_rank` / `human_hot_search` / `human_must_watch` / `human_followed` |
| 漫剧     | `comic_series_rank`  | `comic_series_hot_rank` / `comic_series_hot_play` / `comic_series_new_rank` / `comic_series_hot_search`           |
| AI剧     | `ai_playlet`         | `ai_playlet_hot_sc` 等 6 个                                                                                       |
| 演员     | `ranklist_celebrity` | 无子榜；**响应 video_data=0**（条目是演员形态），客户端剧集 UI 跳过                                               |
| 系列剧   | `series_album`       | `series_album_hot_sc` / `series_album_new_rank`（新季榜）                                                         |

「全部」tab 的 8 个子榜：

| 榜单   | sub_selected_items       |
| ------ | ------------------------ |
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

| 形态 | 触发条件                            | 条目结构                                                                                                                                                                            |
| ---- | ----------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 嵌套 | 匿名请求（无 install_id Cookie）    | `subscribe_data.{series_id,title,vid,score,...}` + 顶层 `category/rec_tags/schedule_publish_time/is_online`                                                                         |
| 扁平 | 带 install_id Cookie 或 target_date | 字段直接在条目上：`item_id,name,cover,item_desc,sub_title_list[],category,categories,schedule_publish_time,expected_publish_time,is_online`；**无 vid**（点击走 seriesId 解析详情） |

首页（不带 target_date）返回默认日（今天）条目 + `calendar_schema.date_list`
（前后各一周）+ `default_date`；条目跨日分布时 `has_more/next_offset` 翻页。

## 4. 找剧（筛选浏览）

```
POST /reading/distribution/category/landpage/v1/   # 与首页推荐同端点，JSON body
```

```json
{
  "client_req_type": 3,
  "filter_ids": "",
  "limit": 18,
  "need_selector_panel": false,
  "offset": 0,
  "req_scene": "default",
  "req_type": "only_content",
  "select_items": {
    "category_dim_epoch": [],
    "category_dim_role": [],
    "category_dim_theme": [],
    "gender": [],
    "genre": [],
    "online_time": [],
    "sort": []
  },
  "session_id": "20261003165900B053FA3876D73D9E20A0"
}
```

**翻页形态（2026-10-07 抓 hgplayer 1.1.6 实锤）**：首页 `offset=0, session_id=""`，
响应下发 `session_id` + `next_offset`；第 2 页起 `offset=next_offset 游标递增`
且 **`session_id` 必须同值回传**（服务端按它记住筛选上下文）——用算术 offset +
空 session_id 翻页会让结果集换源（条数与第三方对不上的根因）。
`limit` 服务端按请求值发放（官方 18，我们用 20/页——probe 实证 20 条足额
返回且 next_offset 跟着走）。

筛选维度：`genre`(体裁) `category_dim_theme`(主题) `category_dim_role`(设定)
`category_dim_epoch`(背景) `sort`(推荐/最新上架/最高热度/最高收藏)
`gender`(受众 男频/女频) `online_time`(7/14/30/90 天内上新)。

**条目标记字段（`video_data[]` 内，2026-10-07 实证）**：`sub_title_list[]`
的 `data_type` 语义：**0=季文本**（「第1季」）、**3=分类**（「古装」，与
category_schema 重复）、**27=热度文本**（「1705万」，官方配火焰图标
`rec_icon_url`）——hgplayer 1.1.6 的剧名上方热度行/季角标同源。
**`tag_info`（对象）是官方运营角标**：`{text:"红果首发/新剧/爆剧…",
icon_url, enable, position}`（剧名前标签）；同名容器在 plan/v 里装的是
「第N季/同IP」。`recommend_info`（JSON 串）含 `rank`（当前列表位次）。

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

### 5.1 输入联想 `GET /reading/bookapi/search/suggest/v`（2026-10-07 抓 1.1.6 实操锁定）

**查询词参数名是 `q`**（不是 search/tab 的 `query`），业务参数逐值对齐：
`q=女帝, bookshelf_search_plan=4, bookstore_tab=16, count=0,
need_personal_recommend=1, need_preload=true, search_source=1, tab_name=feed`。
reading 轻签名 + 匿名 cookie 实测可用（probe_search_suggest 验证）。

响应 `data.query_result_v2[]` 每条：

- `name`（=剧名联想词）、`keyword`（=series_id）、`sug_abstract`
  （「第1季·玄幻·4105万热度」摘要行）
- `search_high_light.rich_text`：`<em>关键词</em>` 高亮形态
- `video_data{series_id, vid, cover(HEIC), title, video_desc}`——带它的条目
  可点击直拨播放；缺它的纯词条目只能回填搜索
- 旧版 `query_result`（纯词数组）与 `suggest_result`（null）同时下发，不用

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
- ⚠️ hgplayer 的预约按钮由 **WebView 前端 fetch** 发出——WebView
  (Chromium) 不吃进程 env 代理，只认**系统代理**（Windows 系统代理 /
  macOS 网络设置），抓它需临时把系统代理指向 mitmproxy
  （操作完记得还原用户的原代理）

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
   - `encrypt_uid` + `event_params{log_id,verify_reason,verify_scene}` +
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

## 用户资料（2026-10-07 实测打通）

```
GET /reading/user/info/v1/    ← lq 域（api5-normal-lq.fqnovel.com）+ 轻签名头 + 登录 cookie
```

- **host 陷阱**：这接口虽属 passport 语义，但挂在 reading 族统一的
  `api5-normal-lq.fqnovel.com`（`LQ_API_ORIGIN` + `api_call_reading`）。
  挂 novel.snssdk.com 上是 404（曾误挂，静默失败三天才发现）。
- 响应 `data` 顶层含 `avatar_url` / `bg_img_url` / `name` / `screen_name`
  / `sec_user_id` 等；`user_info()` 已解析 avatar_url → `AccountState`。
- 另一来源：**sms_login 成功响应的 `data` 顶层同样带 `avatar_url`**（2026-10-04
  抓包实证），登录时直接入库，无需二次请求。
- 会话自检：`cargo test --lib probe_user_info_with_cookies -- --ignored
--nocapture`（`HG_LOGIN_COOKIES="k=v; ..."`），不发短信只读。
- 头像 CDN 是 `p3.douyinpic.com` / `p9-passport.byteacctimg.com`，前端
  CSP `img-src` 已含 `https:`，`<img>` 直连可显。

## 找剧筛选面板（2026-10-07 抓 hgplayer 1.1.5 实操锁定）

```
POST /reading/distribution/category/landpage/v1/   # 与推荐流同端点
{"need_selector_panel":false,"req_scene":"default","req_type":"only_panel"}
```

- 响应 `data.selector_rows[]`：`{type, row_name, items:[{selector_item_id, show_name}]}`
- 八行：`genre`(体裁 short_play/comic_series/ai_series) `category_dim_theme`(主题 29)
  `category_dim_role`(设定 42) `category_dim_epoch`(背景 11) `sort`(推荐
  online_time/hot_score/hot_collect) `gender`(受众 1男/0女) `online_time`(时间 days_7/14/30/90)
  `duration`(长度 duration_0_60/60_120/120_plus)
- ⚠️ **面板按设备下发不齐**：同一份代码，匿名/带 cookie 探测都回 8 行，
  app 设备档案只回 7 行（缺 duration）。但 `select_items.duration` 服务端
  必认（实测 duration_0_60 过滤生效），前端缺行时合成兜底（选项 id 抓包锁定）
- 内容请求（找剧/推荐流同款）：`client_req_type:3 + req_type:"only_content" +
limit:18 + offset + select_items`（每维单元素数组，空选给 `[]`）；
  `session_id` 首页空串、翻页回传
- 响应压缩：landpage 系是 **brotli**（抓包 jsonl 里 resp_body 是
  b64(brotli(JSON))，addon.py 的 safe_text 会 b64 保真）

## 相关作品·系列（2026-10-07 抓 hgplayer 1.1.5 详情页懒加载锁定）

```
GET /reading/bookapi/plan/v?book_id=<series_id>&from=detail_page_more_related
    &scene=10&offset=0&limit=0&need_personal_recommend=1&bookstore_tab=0
    &bookstore_tab_type=0&current_chapter_num=0&total_chapter_num=0&post_id=0
    &is_horizontal_screen=false     ← reading 族轻签名头，lq 域
```

- **懒加载**：hgplayer 打开 series 页不请求，点「相关推荐」tab 才发
- 响应 `data[]` cell 列表：`cell_name:"相关作品"`（14 条：同系列各季
  `tag_info.text`=第1季/第2季… + 同 IP 作品）与 `"猜你喜欢"`（可为空）
- 每项：`series_id/title/cover(~tplv-shrink:640:0.image，扩展名假、内容
JPEG，前端直连 <img> 可显)/score(字符串"8.0")/play_cnt/episode_cnt
(0=未上线→「即将上线」)/video_desc/tag_info.text 角标`
- 未上线（即将上线）条目 hgplayer 卡片带「预约」钮（预约接口已有）
- 我们的落点：`detail.rs fetch_related_series` + `related_series` 命令 +
  详情页「相关作品·系列」卡片行（`series-detail-page.tsx RelatedWorks`）

## 详情页头部数据面（2026-10-08 抓 hgplayer 1.1.3 详情页实锤）

三个数据源拼出与 hgplayer 一致的头部（评分行/热度/徽标/选集时长）：

- **评分 = 剧评接口（9.0 的 series 形态）`data.extra.credibility_score`
  （JSON 数字 8.3）+ `credibility_score_count`（1247）**——与 hgplayer 头部
  「8.3分 1247人评分」逐一吻合。⚠️ 同响应 `extra.book_info.score` **恒为
  空串**（2026-10-07 曾误读它导致评分永远不显示）；`book_info.tags` 是书
  维度标签，与详情头部徽标也不是一套
- **红果热度值 = `video_detail`（第 6 节同端点）的 `hot_score`**（37860097
  → 头部「红果热度值3786万」）；同响应 `video_list[].duration`（秒）是
  选集格「02:35」角标
- 头部徽标行构成：季徽（secondary_infos data_type=0，高亮）+「全 N 集」
  （episode_cnt，前端合成）+ 题材（data_type=3）
- 数字格式化口径（对齐 hgplayer）：万/亿一位小数、整数省小数点
  （27.05万→27.1万、3786.01万→3786万；`formatCountPrecise`）

## 9. 互动操作（2026-10-05 抓 hgplayer 1.1.3 实操全量锁定；2026-10-06 抓 1.1.5 重锁）

全部 **body = gzip JSON**（`Content-Encoding: gzip`）+ reading 轻签名头 +
**登录 cookie + x-tt-token**；重放实证无强签名校验（旧 `x-reading-request`
也过，x-helios/x-medusa 不带也过）。响应 `code==0` 即成功。

⚠️ **2026-10-06 口径漂移事故**：评论区 comment/list 沿用 1.1.3 形态
（business_param 带弹幕字段 + server_channel=1000）被服务端拒
（先报 110001 后报 103001），弹幕形态（comment_source=601）不受影响——
**评论与弹幕在服务端是两套参数校验**，抓 1.1.5 实操重新锁定如下。

### 9.0 评论区列表 `POST /novel/commentapi/comment/list/{group_id}/v1/`

```json
{
  "aid": 8662,
  "business_param": { "book_id": "<series_id>", "need_count": true, "req_type": 0 },
  "comment_source": 4,
  "comment_type": 4,
  "compliance_status": 0,
  "count": 20,
  "cursor": "",
  "group_id": "<vid>",
  "group_type": 30,
  "server_channel": 18,
  "sort": 1
}
```

- 与弹幕形态（见第 5 节 danmaku_payload）的差异：business_param 只有
  `book_id/need_count/req_type` 三字段，`server_channel=18`（弹幕 1000）
- `need_count: true` 让响应 `common_list_info.total` 带评论总数
  （互动栏「💬 N」计数的数据源）
- **回复列表服务端未部署**：`/novel/commentapi/reply/list/` 对 aid=8662
  全 comment_source 均 `cannot found handler`（已穷举路径变体 + source
  0~1500）；hgplayer 1.1.5 二进制里有该路径字符串但实操展开回复**不发
  任何网络请求**——多级回复列表两端都不可用，只有 reply/add 能发
  （hgplayer 同样只显示回复数）

### 9.1 视频点赞 / 取消 `POST /novel/articleapi/do_action/v1/`

```json
{
  "action_category": 1,
  "action_reason_remark": "like_click",
  "action_type": 3,
  "business_param": {
    "book_id": 0,
    "has_aigc_content": false,
    "modify_count": 0,
    "shark_param": {
      "enter_from": "MainFragmentActivity",
      "page_list": "MainFragmentActivity",
      "previous_page": ""
    },
    "video_id": "<series_id>"
  },
  "object_id": "<vid>",
  "object_type": 6
}
```

- `action_type`：**3 点赞 / 4 取消**；`object_type=6`（视频），`object_id=vid`（分集）
- `business_param.video_id` 填的是 **series_id**（字段名与语义不符，照抄）
- 点赞计数在响应 `action_cnt_data["<vid>"]`（服务端延迟统计，常为 0，别信）

### 9.2 评论点赞 / 取消 `POST /novel/commentapi/comment/do_action/v1/`

```json
{"action_type":8,"business_param":{"shark_param":{...}},
 "comment_type":4,"object_id":"<comment_id>","object_type":8}
```

`action_type`：**8 点赞 / 9 取消**；`object_type=8`（评论对象）。
注意与 9.1 是**不同路径**（commentapi vs articleapi）。

### 9.3 弹幕 / 评论发送 `POST /novel/commentapi/comment/add/v1/`

```json
{
  "aid": 8662,
  "business_param": {
    "book_id": "<series_id>",
    "ignore_urge_rule": false,
    "offset": 128912,
    "shark_param": {
      "aid": "8662",
      "enter_from": "MainFragmentActivity",
      "page_list": "MainFragmentActivity",
      "previous_page": "",
      "type": "short_play"
    }
  },
  "commit_source": 1500,
  "data_type": 20,
  "group_id": "<vid>",
  "group_type": 30,
  "text": "..."
}
```

- 与拉取同一评论体系：`group_id`=vid、`group_type=30`、`book_id`=series_id
- **弹幕与普通评论只差三个字段**：

|                       | 弹幕        | 评论 |
| --------------------- | ----------- | ---- |
| data_type             | 20          | 4    |
| commit_source         | 1500        | 3    |
| business_param.offset | 播放位置 ms | 0    |

- **评论形态（data_type=4）business_param 另有四字段**（1.1.5 抓包补齐，
  2026-10-06）：`has_aigc_content:false`、`log_extra:{}`、`preset_text_id:""`、
  `text_feature:{}`，且 shark_param 是**素形态**（不带 aid/type——那俩是
  弹幕专属）。实测带旧形态也能过，但按抓包原样对齐防风控收紧
- 响应 `data.comment_info.comment_id` 是新评论的 id（发弹幕成功后本地
  乐观插入用 `expand.offset_time = offset`）

### 9.3.1 回复 `POST /novel/commentapi/reply/add/v1/`（2026-10-06 抓 1.1.5 锁定）

回复**不走 comment/add 带回复字段**，是独立端点。body 与评论形态同构，
另加顶层 `reply_to_comment_id`（被回复的评论 id）；回复「回复」再加
顶层 `reply_to_reply_id`（响应回显有此字段，多级回复同端点）。
差异字段：`commit_source: 9`（评论 3 / 弹幕 1500）。
响应 id 在 `data.reply.reply_id`（注意不是 comment_info）。

### 9.4 收藏（追剧/书架）`POST /reading/bookapi/bookshelf/video/update/v`

```json
{"is_cancelled":false,"shark_extra":{...埋点},
 "update_bookshelf_video_list":[{"book_id":"<series_id>","book_type":2,
   "modify_time":1791202503400,"video_shelf_operate_type":0}]}
```

- `video_shelf_operate_type`：**0 收藏 / 1 取消**；对象是 **series_id**
  （`book_id` 字段），`book_type=2` 短剧
- shark_extra 补齐（1.1.5）：`inactive_type:"0"`、`is_active_behavior:"true"`
  （字符串形态，与 enter_from 等并列）
- 列表查询是配套的 `GET /reading/bookapi/bookshelf/video/list/v?target_user_id=`
  （target_user_id = 登录 uid；2026-10-06 实测响应 `data.video_shelf_info[]`
  条目字段未在带数据样本中取到——probe 实测我们的容错解析
  （series_id/book_id/item_id 任一）命中 1 条真实收藏）

### 9.4.1 观看进度云上报（2026-10-06 抓 1.1.5 锁定）

官方客户端播片时同时打两个接口（约每分钟 + 切集时）：

**`POST /reading/bookapi/read_history/update/v`** —— `book_id`/`vid` 是
JSON **数字**（精度内直接发数字）：

```json
{
  "update_datas": [
    {
      "book_id": 7690797206416133145,
      "book_type": 2,
      "chapter_index": 0,
      "current_play_position": 10000,
      "digged_count": 0,
      "duration": 0,
      "episode_cnt": 0,
      "is_delete": false,
      "is_interactive_game": false,
      "is_listen_mode": false,
      "is_multi_season": 0,
      "meet_guide_comment_tag": false,
      "origin_novel_book_id": 0,
      "player_accumulate_total_time": 10000,
      "read_timestamp_ms": 1791266476339,
      "recent_reads": 0,
      "retain_video_play_time": 0,
      "season_index": 0,
      "series_play_cnt": 0,
      "tone_id": 0,
      "update_timestamp_ms": 1791266476339,
      "use_soft_delete": false,
      "user_digg": false,
      "user_playlet_comment_flag": false,
      "vid": 7690802329313872921,
      "vid_index": 0
    }
  ]
}
```

**`POST /reading/bookapi/read_progress/upload/v`** —— `book_id`/`item_id`
是**字符串**：

```json
{
  "books": [
    {
      "book_id": "7690797206416133145",
      "book_type": 2,
      "channel_id": 0,
      "check_timestamp": false,
      "cur_channel_id": 0,
      "current_play_time": 10000,
      "is_listen_mode": false,
      "is_local_book": false,
      "item_id": "7690802329313872921",
      "listen_and_read": false,
      "page_index": 0,
      "page_progress_rate": 0,
      "paragraph_offset": 0,
      "player_cumulative_total_duration": 10000,
      "progress_type": 0,
      "read_timestamp_ms": 1791266476339,
      "tone_id": 0,
      "vid_index": 0
    }
  ]
}
```

- hgplayer 传 0 的字段（duration/retain/episode_cnt 等）照抄 0；
  `player_accumulate_total_time` 样本里等于当时进度，我们同用 position_ms
- 我们实现：`history.rs report_watch_progress` 一次打双接口，前端
  persist 节流（~1 分钟 + 切集/退出），匿名后端静默跳过

### 9.5 互动状态列表 `GET /reading/ugc/action/mget/v`

query 业务参数：`action_type=3, count=100, offset=0,
object_type_list=6,15,10`。返回**用户互动过的视频列表**（不是单集查询）：
`data.mixed_data_list[].video_data{vid, series_id, user_digg, digged_count,
user_digg_timestamp_ms, video_detail{followed, followed_cnt, series_title}}`。
用途：打开播放页时 best-effort 回显「已赞/已追」状态（列表 100 条内匹配
vid / series_id）；单集精确查询接口未抓到。

## 已知未抓 / 待做

- 搜索联想 `suggest/v`：**已实现**（`search.rs search_suggest` +
  `search_suggest_cmd`，probe 直连验证通过；找剧搜索框防抖 300ms 联想下拉，
  点击直拨播放）
- 推荐流 badge：**已实现**（`tag_info.text`，见第 4 节条目标记字段）
- 设备注册 `POST log.snssdk.com/service/2/device_register/`：**已实现**
  （`register.rs register_device`，tt_encrypt_v5 加密 body；签名档案须带
  已激活的 device_id、body 用空指纹新号——服务端按 body 指纹发新号），
  但**未接入主流程**（当前静态设备档案可用；待其被风控拒发号时再接）
- 扫码登录：端点未抓到，未实现（短信登录含 MFA 上行短信已全量落地）
- hgplayer 1.1.6 二进制里另有未接端点：`read_history/list/v`（云端历史
  列表）、`comment/del/v1`（评论删除）、`book_pack_fields/v1`、
  `user/share/short_url/`、`read_progress/list|get`——按需再抓

## 抓包数据文件

历史轮次（旧 Windows 抓包机的系统临时目录 `hg_capture/`，易失可能已清，
不再依赖）。

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

重放工作流（Windows / macOS 通用）：

1. `mitmdump -p 8080 -s captures/addon.py`（pip 装 mitmproxy ≥12；
   首次运行在 `~/.mitmproxy/` 生成 CA 并信任进系统——macOS 钥匙串 /
   Windows 证书存储）
2. 从终端以 `HTTP_PROXY/HTTPS_PROXY=http://127.0.0.1:8080` 启动
   hgplayer 1.1.5 可执行文件（Wails/Go 后端吃 env 代理，须从终端带
   env 启动，GUI 方式不继承 shell 环境；路径按本机安装位置）
3. computer-use 驱动 UI 触发目标接口
