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

## 1. 排行榜（8 个榜单）

```
GET /reading/bookapi/bookmall/cell/change/v        # 注意 /v 不带斜杠
```

固定参数：`cell_id=7470092475068071998, tab_type=26, selected_items=all,
category_id=0, cell_sub_id=0, client_req_type=2, client_template=2, gender=2,
limit=0, offset=0, unlimited_selector_change_type=2`

榜单切换参数 `sub_selected_items`：

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

- `addon.py` mitmproxy dump 脚本（目标域名过滤，输出 JSONL，MITM_OUT 可覆盖输出路径）
- `flows-YYYYMMDD.jsonl` 按日分轮的抓包产物（req 头/参数/resp body b64+brotli）

重放工作流：`mitmdump.exe -p 8080 -s captures/addon.py`（mitmproxy 12.2.3 在
`%LocalAppData%\Programs\Python\Python314\Scripts\`，CA 已在
`~/.mitmproxy\` 且系统已信任）→ 以 `HTTP_PROXY/HTTPS_PROXY=http://127.0.0.1:8080`
环境变量启动 `C:\Users\liu13\Downloads\hongguo-v1.1.2-windows-amd64.exe`
（Wails/Go 后端吃 env 代理）→ computer-use 驱动 UI 触发目标接口。
