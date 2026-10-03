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
bookshelf_search_plan=4`

翻页：`offset=N&passback=N&search_id={首响应返回的 search_id}`

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

## 抓包数据文件

`C:\Users\liu13\AppData\Local\Temp\hg_capture\`
- `flows.jsonl` 本轮完整抓包（resp_body b64+brotli）
- `flows_run1.jsonl` / `flows_old.jsonl` 前两轮（resp 截断 6k）
- `rank_must_watch.json` / `rank_hot_sc.json` 已解出的榜单样本
