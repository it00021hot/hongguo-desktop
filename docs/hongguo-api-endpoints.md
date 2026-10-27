# 红果官方 App API 端点清单
> 官方 APK（com.phoenix.read 7.3.9.32）静态提取的全量路径清单（1302 条，含本项目
> 未接的接口域）见 [`hongguo-apk-endpoints.md`](hongguo-apk-endpoints.md)——那份只是
> 路径在场，参数/签名/响应口径仍以本文件抓包为准。

## 0. 公共约定与设备身份
**版本身份**（全仓唯一出处 `hongguo-signer::device` 常量）：

- 业务请求：`version_code/update_version_code/manifest_version_code = 74232`、
  `version_name = 7.4.2.32`、`pv_player` 与 version_code 同值。
- UA：`com.phoenix.read/74232 (Linux; U; Android 14; zh_CN; Xiaomi 14; Build/UKQ1.230804.001;tt-ok/3.12.13.20)`。
  UA 尾段是匿名风控闸门：73932 时代的 `Cronet/TTNetVersion:…` 尾段在
  匿名请求上被静默拒。
- 公共头：`passport-sdk-version: 5051452`、`x-vc-bdturing-sdk-version: 4.0.3.cn`、
  `sdk-version: 2`、`x-ss-req-ticket: {ms}`。`x-tt-request-tag` 与 `x-tt-dt`
  业务请求**不带**，匿名带上会被静默拒。

**设备身份两态**：

| | 匿名档案 | 注册档案 |
| --- | --- | --- |
| 档案字段 | cdid + 机型指纹（无 iid/device_id/openudid） | cdid/openudid/device_id/iid + ttreq 票 |
| cookie | 必须零 cookie（自报未知 install_id 反而被拒） | `install_id={iid}; ttreq={票}`（同源，注册响应 Set-Cookie 下发） |
| 能力 | 信息流/搜索/详情可读，**不可取流**（video_model 回 200+0） | 全量可用；取流按会话新鲜度放行，注册即用即活 |

**设备注册 `POST log.snssdk.com/service/2/device_register/`**（实现
`hongguo-api::register`，73932 口径，`new_user:1` 实测通过）：

- 入参 query：40 参公共指纹——`_rticket, ac, aid, appTheme, app_name,
  app_type, cdid, channel, cpu_support64, cronet_version, device_brand,
  device_platform, device_type, dpi, first_launch_timestamp, host_abi,
  is_android_fold, is_android_pad, is_guest_mode, is_preinstall, language,
  last_deeplink_update_version_code, manifest_version_code, md,
  minor_status, need_personal_recommend, openudid, os, os_api, os_version,
  package, resolution, ssmix, ts, tt_data, ttnet_version,
  update_version_code, use_store_region_cookie, version_code, version_name`
  （版本身份字段覆写 73932/7.3.9.32）。
- 入参头：签名五件套 + `sdk-version: 2` + `x-ss-stub: {body MD5 大写}` +
  `content-type: application/octet-stream;tt-data=a` + `lc: 101` +
  `log-encode-type: gzip` + `x-ss-dp: 8662` + `passport-sdk-version:
  5051452` + `x-vc-bdturing-sdk-version: 4.0.3.cn`；零 cookie。
- 入参 body：TT-Encrypt V5 包 73932 指纹注册 JSON。加密格式（实现
  `tt_encrypt_v5_full`，逐字节锁 `tt_v5_reproduces_accepted_body`）：
  - `packet = magic("tc\x05\x10\x00\x00") ‖ salt(32 随机) ‖ AES-128-CBC(PKCS7(payload))`
  - `derive(salt) = sha512(sha512(salt) ‖ ORD_LIST(64))`；`key =
    derive[0..16]`、`iv = derive[16..32]`（**包内无 IV 字段，iv 不上线**）
  - `payload = sha512(gz) 完整 64 字节 ‖ gz`；gzip 头 XFL/OS 覆写 `00/00`
- 注册 JSON 关键字段（全表见 `reg73932_json_str`）：`device_model: "Xiaomi 14"`、
  `sdk_version: "3.7.3-rc.116-douyin"`、`sig_hash: aea615ab…`、
  `display_name: "红果短剧"`、`custom.client_ipv4 = "116.130.x.x" 随机伪值`
  （服务端不校验真实出口 IP）。身份字段生成规则：
  - `cdid` / `clientudid` / `req_id`：各自独立 UUIDv4。
  - `openudid = md5("{cdid}_{N}").hex()[:16]`，`N ∈ [100000, 999999]`。
  - `apk_first_install_time = now_ms − randInt(100000,500000)`；
    `first_launch_timestamp = now_sec − randInt(10,60)`。
- 出参：`{"server_time","device_id","install_id","new_user","device_id_str",
  "install_id_str","device_token"}` + Set-Cookie `install_id` / `ttreq`。
  `device_id: 0` = 被静默拒（格式或指纹有变）。

**官方设备身份的确定性**：cdid/openudid 由 ANDROID_ID 等设备种子派生
（pm clear 后不变）；服务端对同一指纹恒回同一 install_id——「清数据
不掉身份」的根因。

## 来源与总述
来源：官方 Android App（com.phoenix.read）mitmproxy 抓包实测。业务端点
在 `api5-normal-lq.fqnovel.com`（下称 **lq 域**），签名五件套
（x-gorgon/x-argus/x-ladon/x-helios/x-medusa）+ `x-reading-request: {ticket_ms}-{random}` 头。
响应普遍为 **brotli 压缩 JSON**（请求头带 `accept-encoding: gzip, deflate, br`）。
业务参数一律放 **URL query**（POST 也是），POST body 另有 JSON 业务体。

公共 query 参数（每个请求都带，签名覆盖）：`device_platform=android, aid,
version_code, manifest_version_code, app_name, channel, device_type,
device_id/iid/openudid/cdid, os=android, os_version, dpi=460, resolution,
host_abi=arm64-v8a, _rticket={ms}, …`（即设备档案全量字段 + 时间戳，
见 `hongguo-signer::device::DeviceProfile`）。

**请求形态两族**（对齐时别混）：

|           | lq 域 reading 系                                    | sinfonlineb 域播放/推荐流                  |
| --------- | --------------------------------------------------- | ------------------------------------------ |
| 端点      | 榜单/新剧/日历/预约列表/搜索/弹幕/预约操作          | multi_video_model / landpage               |
| 头        | x-ss-dp + lc + x-reading-request（无五件套）        | 全签名五件套                               |
| POST body | **一律 gzip**（`Content-Encoding: gzip`）           | 未压缩（服务端两种都收）                   |

**参数必须逐值对齐抓包，不能凭「也能用」保留旧值**：`send_code` 的
`type=1` 能发码但被服务端按「换绑」场景处理（目标号已绑定其它账号时
报 1001），短信登录发码场景是 `type=3731`。

**取流端点**：`multi_video_model/player/v1` 必须打 **lq 域**；
sinfonlineb 域与 `preload/v1` 路径回 **200 + 0 字节**（同一 body 换
lq+player 即回完整 video_model）。body 逐值对齐抓包：`dr_scene:"player"`
+ biz_param 含 `disable_video_relate_book:false`。该端点有按设备/维度
的**限频**，身份被消耗尽时同样回 200+0——排障先分辨「形态错」与
「限频」（换一个长时间未打该端点的环境对照）。取流还要求**活的注册
身份**（见第 0 节：匿名/陈旧身份一律 200+0）。

## 1. 排行榜（内容 tab × 子榜 × 筛选面板 × 月榜）
```
GET /reading/bookapi/bookmall/cell/change/v        # 注意 /v 不带斜杠
```

固定参数：`cell_id=7470092475068071998, tab_type=26, selected_items=all,
category_id=0, cell_sub_id=0, client_req_type=2, client_template=2, gender=2,
limit=0, offset=0`（其余定位参数见下）

**翻页协议（2026-10-17 抓官方 App 实流 69 条锁定，每条无一例外）**：

- 每页 **10 条**，响应 `next_offset` 步进 10（旧口径「每页 20 条、相邻页
  重叠 10 条」是早期误读，官方 App 每页 10 条；首屏响应可交付
  11 条——`next_offset` 仍是 10）
- **首屏**（换 tab/子榜/筛选/月榜后的第一批）：`offset=0`、
  **`unlimited_selector_change_type=2`**、不带 `session_id`、不带
  `filter_ids`
- **翻页续载**：`offset=next_offset`、**`usct=1`**、回传响应下发的
  `session_id` + `filter_ids`（本会话已下发的 series_id 逗号表，官方带
  尾逗号）+ `rank_version`（见下）。**续载带 usct=2 会被服务端当「换一批」
  处理——滚动加载重复/错乱的根因**
- ⚠️ **`filter_ids` 是全局排异（2026-10-17 实验实锤）**：带满 fid 的旧
  窗口重放，服务端不是重发原窗口，而是**跳过全部已见、从未见段接着发**
  （滚 3 页后重放 offset=10 → 回第 32~41 位）。因此该协议只能顺序增量，
  **任何带旧游标的全页重放都会跳档**（前端 staleTime=0 重挂载重拉正是
  这个形态，曾致「中间缺 20 条」）。项目的对策：`rank_list` 后端持有
  会话累积账本，对外**窗口语义**——同 (session, offset) 重放恒得同批
  条目（幂等），账本不够深才向上游顺序补页；`recommend_info.rank` 是
  页内位次（可重复缺号），连续名次由客户端按拼接序号自算
- `rank_version`：响应随行下发（data 与 cell_view 两层同值，JSON 数字），
  客户端全局记最新值、后续请求原样回传（字符串形态）

**月榜（榜单历史，2026-10-17 抓必看榜实测）**：月榜型子榜（必看/收藏/
臻果…）的选项表在响应 `cell_selector.outer_row.items[].sub_cell_selector.
outer_row.items[].background_selector.inner_rows[]`，与 panel_selector
同构：综合行 `date_all`（总榜）+ 年份行 `date_YYYYMM`（2026年 10月…
2025年 …）。选中值经 **`background_selected_items=date_YYYYMM`** 回传
（2026年9月 = `date_202609`），与 `panel_selected_items` 相互独立、可
同时携带；默认（当前月）可不传。

### 1.0 首页推荐流
信息流首页的**两段式**数据源（本仓 `fetch_recommend_feed` 同构）：

**首页（切 tab）**：`GET /reading/bookapi/bookmall/tab/v?client_req_type=4&tab_type=<T>`
（query 另带 bottom_tab_type=7/tab_index=0/screen_width_px=1078/stream_count 等 18 项
业务参数——与官方 cr=4 样本逐字段一致，其余设备参数由设备档案注入）

- ⚠️ **官方手机 App 的形态分场景**（2026-10-25 冷启动抓包复核 + 2026-10-15 实流）：
  启动首拉顶栏 `tab_type=16&client_req_type=1028`（响应只有 4 条 tab_item，
  **不带 feed**，首屏列表走本地缓存）；`tab_type=-1&client_req_type=3` 是
  按 bottom_tab 域取全部 tab 的通用查询；**切频道才发 `cr=4&tab_type=<T>`**
  （响应 tab_item 自带该频道 feed，本仓两段式即此形态）；信息流刷新走
  `cell/change?client_req_type=2`。注意点频道 tab 会自动续播上次看的剧
  （直接进沉浸页），抓不到 cr=4 不代表它没发
- tab 表：**关注 45 / 漫剧 36 / 真人剧 39 / 推荐 16**，`tab_item[T]` 自带
  `bookstore_id`（=cell/change 的 plan_id，**每 tab 不同**）+ 会话 `session_id`
- ⚠️ **cell 要按内容选，不能按位置取**：`cell_data[]` 里 [0] 可能是空的
  运营金刚位卡（show_type=583，tab36 实测），推荐流是**嵌套
  `cell_data[].video_data[]` 非空**的那个 cell（cell_name=「猜你喜欢」，
  show_type=407，其 cell_id 即 cell/change 的 cell_id）
- 用推荐的 id 打 tab39 服务端回落混合池（全 1004 漫剧）——「真人 tab 全是
  漫剧」的根因；取错 cell 则翻页恒空——「漫剧 tab 空批」的根因

**翻页**：`GET /reading/bookapi/bookmall/cell/change/v`（cell_id + plan_id 用
目标 tab 自己的）+ 三件套：`session_id`（首页下发）+ `offset`（**上一页响应
的 next_offset**，0→6→12 递进）+ `filter_ids`（已下发过的 series_id 逗号表，
服务端排除已见；抓包实锤翻页 0 重复）

- 响应 `data`：`cell_view.cell_data[].video_data[]`（条目身份在
  `video_detail`：series_title/series_cover(_uri)/series_id_str/episode_cnt/
  series_play_cnt/content_type/category_schema）+ has_more/next_offset/session_id
- **展示字段**（2026-10-08 实操抓包逐一对齐）：
  - 热度行（火焰图标+文案）= 条目级 `rec_tags[]` 的 `data_type=1` content
    （「共217万人在追」「真人剧新番榜 No.4」「4920万热度」，多段拼接）
  - 运营角标 pill = `video_detail.tag_info.text`（「新剧」，官方随 bg_color）
  - `secondary_infos`：data_type 缺省/0=季徽（「第1季」）、3=题材、4=评分
    （「9.6分」）
- 内容按时间桶在服务端轮换，「换一批」= 新会话重拉（filter_ids 清空）
- 我们的落点：`rank.rs fetch_recommend_feed`（两段式 + 配置进程缓存 +
  抓包已知值兜底）+ `recommend_feed` 命令 + `useFeed` 三件套翻页
- ⚠️ **翻页必须带 `filter_ids`**（2026-10-25 官方 App 7.3.9.32 实流再锤：
  翻页请求 `offset=0` + session + filter_ids=已刷过的 series_id；GPUI 壳
  曾漏传——「沉浸流一直重复刷刷过的剧」的根因，服务端不做会话内去重）
- ⚠️ **取流批量双形态**（2026-10-25 抓 7.3.9.32 实流）：
  ① `multi_video_detail/preload`：`series_id` 是**逗号串**（6 部一发），
  响应 `data[series_id]` 按剧分发（起播后 ~1s，与 ② 同发）；
  ② `multi_video_model/preload`（`dr_scene:"preload"`）：**纯滑动预取**
  场景（半屏滑动未起播时）；`multi_video_model/player/v1`
  （`dr_scene:"player"`）：**起播/切剧时刻**（6 vid：当前 + 信息流后续）。
  两形态 biz_param 全同、**无 `from_video_id`/`source` 键**；
  `mixed_video_id_map` 按**条目 content_type 原样串**分组（直连 probe
  实证：key 错配 → 空响应；漫剧 feed ct=1004→"1004"、真人剧
  ct=1→"1"、书城 cell 条目 1001→"1001"）。
- ⚠️ **剧内切集/单集取流走 `video_model/v1/`**（2026-10-25 官方切集实流，
  急诊摸鱼王 ep37）：`content_type` = **这一集所属剧的真实类型，随剧变**
  （官方样本 1004/1/1001 各配其剧，随剧变，不是协议
  常量）、`source:4` 在场（批量体无此键）、`video_platform:3`、
  `from_video_id:""`。另有一冷启动预览形态（`video_platform:0` +
  `use_server_dns:true` + `use_server_dns_scene:"cold_start_stage"`，
  半屏预览流专用，本仓无该场景）。
- 本仓链路（`play_url.rs`）：预取暂存命中直接用 → 批量 player（起播
  形态，注册表分组）→ 单集回落（content_type 取注册表，未知填占位
  1004——服务端对该字段容错实测）。vid→content_type 注册来源：推荐流
  解析（入口 vid）、分集档案解析（整剧各集，`video_data.content_type`）、
  批量分组、壳起播（整剧注册）。
- ⚠️ **不要用 landpage（找页筛选端点）当首页推荐**：匿名设备永远回
  同一批静态默认列表，且翻页不回传 session 换源重复——首页推荐必须走
  本节 bookmall 端点。

### 1.1 内容 tab 与子榜
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
面板选中项用 `panel_selected_items` 传值，**单值**——抓包实测
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

**切日期：`target_date=YYYYMMDD`**（2026-10-04 抓包切日期锁定；此前试过的
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

**翻页形态（2026-10-07 抓包实锤）**：首页 `offset=0, session_id=""`，
响应下发 `session_id` + `next_offset`；第 2 页起 `offset=next_offset 游标递增`
且 **`session_id` 必须同值回传**（服务端按它记住筛选上下文）——用算术 offset +
空 session_id 翻页会让结果集换源（条数与官方 App 对不上的根因）。
`limit` 服务端按请求值发放（官方 18，我们用 20/页——probe 实证 20 条足额
返回且 next_offset 跟着走）。

筛选维度：`genre`(体裁) `category_dim_theme`(主题) `category_dim_role`(设定)
`category_dim_epoch`(背景) `sort`(推荐/最新上架/最高热度/最高收藏)
`gender`(受众 男频/女频) `online_time`(7/14/30/90 天内上新)。

**条目标记字段（`video_data[]` 内，2026-10-07 实证）**：`sub_title_list[]`
的 `data_type` 语义：**0=季文本**（「第1季」）、**3=分类**（「古装」，与
category_schema 重复）、**27=热度文本**（「1705万」，官方配火焰图标
`rec_icon_url`）——官方 App 剧名上方热度行/季角标同源。
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

### 5.1 输入联想 `GET /reading/bookapi/search/suggest/v`
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

## 7. 预约 / 取消预约
```
POST /reading/bookapi/search/uncover_subscribe/v
```

- **body 是 gzip 压缩的 JSON**（头带 `Content-Encoding: gzip`）：
  `{"item_id": <series_id>, "item_type": 1, "op_type": 1 预约 / 2 取消,
"shark_param": {埋点上下文，不校验}, "wish_list_all_del": 0}`
- query 只放设备指纹；reading 轻签名头（x-ss-dp/lc/x-reading-request）；
  **必须带登录 cookie**，响应 `code==0` 即成功
  （操作完记得还原用户的原代理）

## 8. 短信登录（passport 系）
```
POST /passport/mobile/send_code/v1/   mobile 密文 + type
POST /passport/mobile/sms_login/      mobile/code 密文 + csrf cookie
POST /passport/upsms/verify/          MFA 上行短信轮询（form body）
```

> 扫码登录不可用：抖音开放平台 10015（client_key 不放行 web 扫码）+
> passport 16（aid=8662 无 web 扫码权限）——服务端权限墙，仅短信登录
> （含 MFA 上行短信）可走。

- `mobile` / `code` 都是 **XOR(0x05) hex 密文**；mobile 密文须带
  **`+86` 国码前缀**（不带按残缺号处理）；`mix_mode=1`
- 发码响应 **Set-Cookie `passport_csrf_token`** = 会话绑定凭据，
  sms_login 的 cookie 带上它（body **不传** mobile_ticket——抓包实测）
- `type`：发码场景参数，1 与 3731（实测值）都受理
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
     （宁夏银川 95 扩展号段）——**回复到它服务端收不到**，真实可回复
     通道是 `10691859839103`（运营商 106 网关；抓包全量数据中
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
4. 退出登录**无网络请求**（纯本地清账号）

## 会话有效期与 x-tt-token
- **sessionid 有效期 60 天**：`sid_guard = <sessionid>|<登录时间戳>|5184000|<Expires>`
  （5184000s = 60 天）。手机"登录一次在线很久"的直接原因。
- **续约**：客户端侧无任何 refresh/auth 端点（passport 路径只有
  send_code/sms_login/upsms）——token 里的 refresh 段不被主动使用，
  最可能是服务端对活跃用户在响应头滑动续期（抓包 2h 窗口内凭据无变化，
  未实证；将来可在 addon 记录响应头验证）。过期后重新短信登录。
- **x-tt-token 双 token**：登录**响应头**下发 `00<access 195位>--<refresh
150位>-3.0.3`（Set-Cookie / body 里都没有）；请求带**前 56 位短形式**
  （`00`+sessionid+22 位），官方 App 每个请求都在场。我们已对齐：
  `AccountState.token` 落库 → `ApiEnv.x_tt_token` → `send_once` 注入头。
  旧账号该字段为空（不带），下次登录自动补上。
- 登录态完整凭据：sessionid / sid_tt / sessionid_ss（同值）+ sid_guard +
  uid_tt(_ss) + odin_tt + d_ticket + n_mh + session_tlb_tag 等 17 个 cookie
  （`extract_cookie_pairs` 全收，同名后值覆盖）+ x-tt-token 头凭据。

## 用户资料
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

## 找剧筛选面板
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

## 相关作品·系列
```
GET /reading/bookapi/plan/v?book_id=<series_id>&from=detail_page_more_related
    &scene=10&offset=0&limit=0&need_personal_recommend=1&bookstore_tab=0
    &bookstore_tab_type=0&current_chapter_num=0&total_chapter_num=0&post_id=0
    &is_horizontal_screen=false     ← reading 族轻签名头，lq 域
```

- **懒加载**：打开 series 页不请求，点「相关推荐」tab 才发
- 响应 `data[]` cell 列表：`cell_name:"相关作品"`（14 条：同系列各季
  `tag_info.text`=第1季/第2季… + 同 IP 作品）与 `"猜你喜欢"`（可为空）
- 每项：`series_id/title/cover(~tplv-shrink:640:0.image，扩展名假、内容
JPEG，前端直连 <img> 可显)/score(字符串"8.0")/play_cnt/episode_cnt
(0=未上线→「即将上线」)/video_desc/tag_info.text 角标`
- 未上线（即将上线）条目卡片带「预约」钮（预约接口已有）
- 我们的落点：`detail.rs fetch_related_series` + `related_series` 命令 +
  详情页「相关作品·系列」卡片行（`series-detail-page.tsx RelatedWorks`）

## 详情页头部数据面
三个数据源拼出与官方 App 一致的头部（评分行/热度/徽标/选集时长）：

- **评分 = 剧评接口（9.0 的 series 形态）`data.extra.credibility_score`
  （JSON 数字 8.3）+ `credibility_score_count`（1247）**——与官方 App 头部
  「8.3分 1247人评分」逐一吻合。⚠️ 同响应 `extra.book_info.score` **恒为
  空串**（2026-10-07 曾误读它导致评分永远不显示）；`book_info.tags` 是书
  维度标签，与详情头部徽标也不是一套
- **红果热度值 = `video_detail`（第 6 节同端点）的 `hot_score`**（37860097
  → 头部「红果热度值3786万」）；同响应 `video_list[].duration`（秒）是
  选集格「02:35」角标
- 头部徽标行构成：季徽（secondary_infos data_type=0，高亮）+「全 N 集」
  （episode_cnt，前端合成）+ 题材（data_type=3）
- 数字格式化口径（对齐官方 App）：万/亿一位小数、整数省小数点
  （27.05万→27.1万、3786.01万→3786万；`formatCountPrecise`）
- **出品方 / 演职员 = `video_detail` 的 `data.video_data.main_creator_users[]`**
  （`base_info.{user_id,user_name,user_avatar}` + `user_tag.sub_title_list[0].text`
  出角色文案；⚠️ 匿名设备实测**该字段不返回**，登录态才有）。

  ⚠️ **`user_avatar` 是「签名 http」地址**（2026-10-25 实包）：
  `http://p3-reading-sign.fqnovelpic.com/novel-pic/<hash>~tplv-s85hriknmn-jpeg-v1:144:0.jpeg?lk3s=...&x-expires=...&x-signature=...`
  ——服务端 http/https 都回 200（同一签名），但**浏览器侧 http 会被 CSP
  `img-src`（无 `http:`）与混合内容拦掉** → `<img>` 空白。所以 Tauri 壳这处
  必须过 `normalizeAvatarUrl`（http→https；见 `src/utils/image-url.ts`），
  与评论/弹幕/@/创作者档案头像同口径；漏一处就是「详情页创作者筹码没头像」。
  GPUI 壳不走浏览器，另有一道档位坑（见 `docs/gpui-kit-research.md`）。

## 9. 互动操作
全部 **body = gzip JSON**（`Content-Encoding: gzip`）+ reading 轻签名头 +
**登录 cookie + x-tt-token**；重放实证无强签名校验（旧 `x-reading-request`
也过，x-helios/x-medusa 不带也过）。响应 `code==0` 即成功。

**评论与弹幕在服务端是两套参数校验**：评论区 comment/list 用弹幕形态
（business_param 带弹幕字段 + server_channel=1000）会被拒（110001/103001），
弹幕形态（comment_source=601）不受影响——两族参数不可混用，以下按
实操抓包记录。

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
- ~~回复列表服务端未部署~~ **2026-10-10 推翻**：新版客户端起回复
  列表可用（独立端点 reply/list，见 9.0.1）；更早「无 handler」
  是服务端后上线了该能力

### 9.0.1 回复列表 `POST /novel/commentapi/reply/list/{comment_id}/v1/`
**路径参数是被展开的评论/剧评 id**（不是 group_id）。两个维度同一端点
但请求形态两套，逐字段照抄：

**单集评论维度**（group_type=30，`group_id`=vid）：

```json
{
  "aid": 8662,
  "business_param": { "book_id": "<series_id>", "need_count": false },
  "comment_id": "<comment_id>",
  "comment_source": 504,
  "comment_type": 4,
  "compliance_status": 0,
  "count": 10,
  "cursor": "",
  "group_id": "<vid>",
  "group_type": 30,
  "server_channel": 18
}
```

**剧评维度**（group_type=1，`group_id`=series_id）：

```json
{
  "business_param": { "book_id": "<series_id>", "need_count": true, "real_level": 2 },
  "comment_id": "<comment_id>",
  "comment_source": 501,
  "comment_type": 2,
  "count": 10,
  "group_id": "<series_id>",
  "group_type": 1,
  "server_channel": 34
}
```

- **两维度分叉点**：comment_source（504 评论 / 501 剧评）、comment_type
  （4/2）、server_channel（18/34）、顶层 aid（评论维度带，剧评维度不带）、
  compliance_status（只有评论维度带）、business_param（剧评多 `real_level:2`）
- **cursor 纪律**：评论维度首页是**显式空串**；剧评维度**首屏不带 cursor
  键**（与 comment/list 剧评形态同款纪律）。翻页都是把响应
  `comment_list_info.cursor` 的数字串（"10"/"20"…）原样回传；剧评维度
  `need_count` 首页 true、翻页 false（两维度翻页形态都抓包实证）
- **响应结构与 comment/list 不同**：`data.{comment, comment_list_info,
extra, reply_list}`——分页在 `comment_list_info.{cursor,has_more,total}`
  （不是 common_list_info），条目在 `reply_list[]`（不是 data_list）
- **回复条目**：`{Common, expand, reply_id, reply_to_comment_id,
reply_to_reply_id, reply_to_user_info, stat, sub_reply, user_action}`——
  ⚠️ 回复体在键名**大写的 `Common`** 下（上游序列化怪癖，解析兼容大小写）；
  `Common.content.text`（含 `[表情]` 代码）、`Common.user_info.base_info`、
  `Common.create_timestamp`；点赞数在顶层 `stat.digg_count`、我的点赞态在
  顶层 `user_action.user_digg`（不在 Common 里）
- `reply_to_user_info.base_info.user_name` 是被回复人昵称（「回复 @xxx」）；
  `reply_to_reply_id` 非空 = 这是对「回复」的回复（多级）
- 列表数据源（数据面字段）：`expand.score`/`score_suffix_text` 也在
  `data.comment.expand`（剧评本体）在场

### 9.0.2 回复的点赞 = 评论点赞同款
对一条**回复**点赞就是 9.2 的评论形态（`object_type=8`、`comment_type=4`、
business_param 带 shark_param），`object_id` 传 **reply_id**——实测抓包
（回复 id 7693411634417959705 的赞/取消走的就是这形态）。无独立端点。

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
**复核（2026-10-10 抓包）**：形态与此前样本逐字段一致，响应
`code=0, data=null`；对**回复**点赞也是这形态（object_id 传 reply_id，
见 9.0.2）。

### 9.2.1 剧评点赞 / 取消
同端点换剧评形态，**三处与评论点赞不同，照抓包别合并**：

```json
{
  "action_type": 8,
  "business_param": {},
  "comment_type": 2,
  "object_id": "<review_id>",
  "object_type": 2
}
```

- `object_type=2`（评论是 8）、`comment_type=2`（评论是 4）、
  **`business_param` 是空对象**（评论带 shark_param）
- 取消点赞 = `action_type: 9`，其余不变（抓包实锤）

### 9.3.0 弹幕拉取滚窗协议 `POST /novel/commentapi/comment/list/{vid}/v1/`
弹幕形态（`comment_type=20 / comment_source=601 / server_channel=1000 /
group_type=30 / count=90`）不变，但**不是一次拉全，是 30 秒窗滚动**：

- **首窗**：`business_param.start_offset_time` = 起播位置（ms），
  `cursor=""`；`business_param` 只有
  `book_id / start_offset_time / playlet_item_duration / need_danmaku_guide_type:[]`
  四键，`playlet_item_duration` 是**本集真实时长**（video_model 的
  video_duration，例 199000——旧口径写死 60000）
- **推进**：响应 `common_list_info.cursor`（JSON 串）**原样回传**，请求
  `start_offset_time` 推进到其 `end_offset_time`（[0,30000) → 下窗 30000）；
  每到窗口边界拉下一窗（官方实流 +30s 一发）
- cursor 内带 `danmaku_window_count`（每窗弹幕数，服务端下发）
- ⚠️ **设备风控分决定弹幕量**：`x-medusa`/`x-helios` 是被服务端真实校验
  的风控头（重放实验：换值/去掉即拒；x-ladon/x-argus 不校验）。我们的
  签名占位实现整包自洽能过验签，但被判低信任档——**每窗只回少量条目即
  has_more=false**（热剧实测 6~45 条/窗；官方真机 55 条起步且按窗口
  计数 200+）。逐窗滚动拉取下每窗都有产出，仍优于一次拉全；全量需真
  medusa 载荷（native 风控 SDK，暂无路线）
- ⚠️ 传输形态（登录态/Cookie/gzip body/klink_egdi/设备档案/UA）经重放
  变量隔离**全部无关**——分流只在签名头内容本身

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

- **评论形态（data_type=4）business_param 另有四字段**（抓包补齐，
  2026-10-06）：`has_aigc_content:false`、`log_extra:{}`、`preset_text_id:""`、
  `text_feature:{}`，且 shark_param 是**素形态**（不带 aid/type——那俩是
  弹幕专属）。实测带旧形态也能过，但按抓包原样对齐防风控收紧
- 响应 `data.comment_info.comment_id` 是新评论的 id（发弹幕成功后本地
  乐观插入用 `expand.offset_time = offset`）

### 9.3.2 剧评（整剧评论）列表 + 发送
**列表** = 同端点换形态：`POST /novel/commentapi/comment/list/<series_id>/v1/`，
body 与 9.3 拉取同族但维度换剧：`group_id=<series_id>`、`group_type=1`、
`comment_source=1`、`comment_type=2`、`server_channel=34`、`count=10`、`sort=1`、
business_param 照抓包全量。**首屏不带 cursor 键**；翻页回传上一页响应
`common_list_info.cursor` 的 JSON 串原样：`{"session_id":"…","offset":10}`
（每页 +10，服务端按 session 维持上下文）。

响应：`common_list_info.{total, has_more, cursor, log_extra}` +
`data_list[].{comment, data_type}`；每条评分在 `comment.expand.score`
（**十分制字符串**，"7"/"9"/"10"）与 `comment.expand.score_suffix_text`
（"观看1小时后点评"——官方按观看时长生成的后缀）；列表级
`data.extra.book_info.score` = **剧均评分**（"8.6"；空串 = 暂无评分）、
`data.extra.score_cnt` = 评分人数、`collectors_count`。

**发送** = `POST /novel/commentapi/comment/add/v1/`，形态与评论/弹幕完全
不同（照抄抓包，一个字段都别改）：

```json
{
  "business_param": {
    "aigc_template_id": "",
    "aigc_template_text": "",
    "book_id": "<series_id>",
    "comment_tag_list": [],
    "from_famous_comment_id": 0,
    "has_aigc_content": false,
    "ignore_urge_rule": false,
    "is_confirm_request": false,
    "offset": 0,
    "read_item_cnt": 0,
    "score": 10, // ★ 十分制评分（5 星 ×2）随发送走
    "support_para_audio_play": false,
    "text_feature": {},
    "video_is_muted": 0
  },
  "comment_type": 0, // 评论 4/弹幕 20——剧评是 0
  "commit_source": 12, // 评论 3/弹幕 1500/回复 9——剧评是 12
  "data_type": 2, // 评论 4/弹幕 20——剧评是 2
  "group_id": "<series_id>",
  "group_type": 1,
  "image_data": [],
  "rich_text": [],
  "text": "…"
}
```

**发送侧不得混入列表侧参数**（comment_source/comment_type/server_channel）
或回复侧 `commit_source=9`：服务端返回 **103008「无社区功能」**——报错
语义与参数错误毫无关系，只能逐字段对照抓包排除。

### 9.3.1 回复 `POST /novel/commentapi/reply/add/v1/`
回复**不走 comment/add 带回复字段**，是独立端点。body 与评论形态同构，
另加顶层 `reply_to_comment_id`（被回复的评论 id）；回复「回复」再加
顶层 `reply_to_reply_id`（被回复的那条回复 id，多级同端点——2026-10-10
抓包两维度都实证该键）。
差异字段：`commit_source: 9`（评论 3 / 弹幕 1500）。

**响应是完整回复对象回显（2026-10-10 解剖，重要）**：`data.reply` 与
reply/list 条目**同构**——`Common.{content.text, create_timestamp,
user_info.base_info.{user_id 数字, user_name, expand_user_avatar}}`、
`stat`、`user_action`、`reply_id`、`reply_to_*` 全带（user_tag 还带
「我」标记）。官方 App 把这个对象**直接插进回复列表**当 UI 数据源
——只取 reply_id 手拼条目会缺头像/昵称/userId（删除按钮判定失效），
效果与官方 App 两样（本项目 2026-10-10 返工实录）。comment/add 同理：
响应 `data.comment_info` 是完整评论对象（含 `expand.score` 剧评评分
回显），前端直插列表。

**剧评回复（2026-10-10 抓包实操锁定）**——同端点换剧评
形态，与单集评论回复的差异照抄：

```json
{
  "business_param": {
    "book_id": "<series_id>",
    "from_famous_comment_id": 0,
    "has_aigc_content": false,
    "ignore_urge_rule": false,
    "is_confirm_request": false,
    "log_extra": {},
    "offset": 0,
    "read_item_cnt": 0,
    "score": 0,
    "support_para_audio_play": false,
    "text_feature": {},
    "video_is_muted": 0
  },
  "commit_source": 13,
  "data_type": 2,
  "group_id": "<series_id>",
  "group_type": 1,
  "reply_to_comment_id": "<review_id>",
  "text": "…"
}
```

- `commit_source=13`（评论回复 9 / 剧评本体发送 12 / 剧评回复 13——
  四个值各管一摊，别混）；`data_type=2`、`group_type=1`
- **顶层不带 aid**（评论维度回复带 `aid:8662`，剧评维度不带）
- business_param 是剧评发送同款全量字段组，`score` 恒 0（评分只随
  剧评本体走）；回复「回复」加顶层 `reply_to_reply_id`（抓包实锤）
- 表情以 `[名字]` 文本码直接进 `text`（`[爱慕]` 实测入库原样），
  服务端无表情特殊字段

### 9.3.3 删除自己的评论 / 剧评 / 回复 `POST /novel/commentapi/comment/del/v1/`
```json
{ "comment_id": "<id>", "data_type": 9, "service_id": 2 }
```

- `data_type` **恒 9**（删除语义，与发送侧 data_type 无关）
- `service_id`：**2 = 剧评，4 = 评论/回复**（两例抓包；与列表响应里
  `common.service_id` 的取值同源）
- 响应 `code=0` + `BaseResp.StatusCode=0` 即成功，data 为 null

### 9.4 收藏（追剧/书架）`POST /reading/bookapi/bookshelf/video/update/v`
```json
{"is_cancelled":false,"shark_extra":{...埋点},
 "update_bookshelf_video_list":[{"book_id":"<series_id>","book_type":2,
   "modify_time":1791202503400,"video_shelf_operate_type":0}]}
```

- `video_shelf_operate_type`：**0 收藏 / 1 取消**；对象是 **series_id**
  （`book_id` 字段），`book_type=2` 短剧
- shark_extra 补齐（抓包）：`inactive_type:"0"`、`is_active_behavior:"true"`
  （字符串形态，与 enter_from 等并列）
- 列表查询是配套的 `GET /reading/bookapi/bookshelf/video/list/v?target_user_id=`
  （target_user_id = 登录 uid；2026-10-06 实测响应 `data.video_shelf_info[]`
  条目字段未在带数据样本中取到——probe 实测我们的容错解析
  （series_id/book_id/item_id 任一）命中 1 条真实收藏）

### 9.4.1 观看进度云上报
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

- 官方实流传 0 的字段（duration/retain/episode_cnt 等）照抄 0；
  `player_accumulate_total_time` 样本里等于当时进度，我们同用 position_ms
- 我们实现：`history.rs report_watch_progress` 一次打双接口；**触发在播放器
  的上报心跳里**（`history-cadence.ts` / `history_cadence.rs` 两壳同规则，
  见 §9.4.2），匿名后端静默跳过。
  ⚠️ **vid_index 基准（2026-10-17 定）**：线上协议是 **0 基**（样本
  第 1 集 = 0），域值（前端/本地存储）是 1 基——写侧统一 -1 换算
  （`wire_vid_index`，删除形态同源），读侧 +1 转回。曾把 1 基直发
  线上，与官方 App 会话混存后跨端续播差一集

### 9.5 互动状态列表 `GET /reading/ugc/action/mget/v`
query 业务参数：`action_type=3, count=100, offset=0,
object_type_list=6,15,10`。返回**用户互动过的视频列表**（不是单集查询）：
`data.mixed_data_list[].video_data{vid, series_id, user_digg, digged_count,
user_digg_timestamp_ms, video_detail{followed, followed_cnt, series_title}}`。
用途：打开播放页时 best-effort 回显「已赞/已追」状态（列表 100 条内匹配
vid / series_id）；单集精确查询接口未抓到。

### 9.6 表情域
官方评论输入框的表情面板已是**四档**（面板图标：🔍 搜索 / 😀 系统表情 /
♡ 自定义 / GIF 第三方）。前三档与发送形态逐条如下。

#### 9.6.1 表情列表 `GET /reading/ugc/comment/emoticon/v`
query 业务参数：`count` / `offset` / `search_type` / `keyword`（`keyword` 仅
搜索档与自定义档带）。`search_type` 四档（抓包逐档实证）：

| search_type | 官方面板 tab id | keyword | 抓包实证 |
| --- | --- | --- | --- |
| 1 | （搜索页，独立入口） | 用户输入（**带 `+` 前缀 = 前缀匹配**） | 逐字输入时抓到 `+c` → `+ca` → `+cat` |
| 2 | 服务端动态 tab（GIF） | 无 | `count=100&offset=0`，`has_more=true&next_offset=100`（分页） |
| 3 | `emoticon` | 无 | `count=99&offset=0`；**收藏前后都返回空列表**，面板无对应入口 → 本仓不实现 |
| 4 | `profile`（♡ 我的表情） | `profile`（恒定） | `count=49&keyword=profile&offset=0` |

- 官方每次打开表情面板会**并发**拉 2 / 3 / 4 三档（抓包时间戳同毫秒）
- 官方 APK 逆向（`KmpSaasEmojiViewModel`，2026-10-25）确认固定 tab id 是
  `emoji`（本地系统表情，**无网络请求**）/ `profile` / `emoticon`，其余 tab
  由服务端下发
- ♡ 档（`search_type=4`）是**我的表情**：我上传的与我收藏的在**同一列表**
  （实抓：收藏一个官方 GIF 后该档条目由 2 条变 3 条）

响应 `data`：`{info_list:[...], has_more:bool, next_offset:int}`。条目字段：

```json
{
  "id": "6996174263220784397",
  "web_uri": "https://p26-im-emoticon-sign.byteimg.com/xxx~tplv-3jr8j4ixpe-resize:450:450.jpeg?lk3s=…",
  "dynamic_url": "https://p26-im-emoticon-sign.byteimg.com/xxx~tplv-3jr8j4ixpe-resize:450:450.gif?lk3s=…",
  "width": 450, "height": 450, "format": "", "image_type": 2,
  "thumb_nails": [{"web_uri": "", "dynamic_url": "…resize:100:100.gif", "width": 100, "height": 100, "format": ""}],
  "image_name": ""
}
```

- `image_type`：**1 = 静态图 / 2 = 动图**（第三方 GIF 档恒 2）
- `format` 与 `image_name` 官方条目**恒空串**（别当缺省值省掉）
- 自定义表情（`search_type=4`）的条目另带 `expand_web_url`（展开大图）
- 官方面板在第三方档顶部标注「内容来自于第三方」

#### 9.6.2 图片上传 `POST /reading/reader/upload/picture/`
- query：`need_encrypt=0`
- body：**multipart/form-data**（**不 gzip**——reading 族其余接口才 gzip），
  字段名 `picture`，`Content-Type: application/octet-stream`，
  `Content-Transfer-Encoding: binary`，带 `Content-Length`
- 响应：`{"code":0,"web_uri":"tos-cn-i-<bucket>/<hash>",
  "data":"https://p0-reading-private.fqnovel.com/…~tplv-s85hriknmn-jpeg.jpeg?lk3s=…&policy=…"}`
  ——`data` 是可直接引用的完整签名 URL（评论 `image_data.web_uri` 填它），
  `web_uri` 是对象 key

#### 9.6.3 添加 / 删除「我的表情」 `POST /reading/ugc/comment/emoticon/collect/v`
body（**扁平** JSON，6 个字段恒在场；gzip；官方按 key 字母序序列化）：

```json
{"collect_type":1,"image_height":449,"image_id":"7305929666710342938",
 "image_type":2,"image_url":"https://p26-im-emoticon-sign.byteimg.com/…webp?…",
 "image_width":425}
```

三种实抓形态（2026-10-25）：

| 场景 | collect_type | image_id | image_url | image_type |
| --- | --- | --- | --- | --- |
| ♡ 档「+」上传本地图片 | 1 | `""` | 9.6.2 返回的 `data` 完整签名 URL | 1 |
| 长按表情 / 长按评论附图「添加到表情」 | 1 | **源表情 id** | 该表情的静态 URL | 1 / 2 |
| 长按 ♡ 档里的表情「删除表情」 | 2 | 我的表情 id | 我那份表情的 `web_uri` | 1 / 2 |

- `image_url` 取值照官方 `ImageData` 拼接规则（`oi2.s.o`）：
  `dynamic_url ?: web_url ?: web_uri`（`expand_web_url` 不参与）。实包里
  收藏动图表情传的仍是**静态** `web_uri`（`image_type` 仍是 2）
- 响应：添加回 `data.image_data` = 落库后的表情对象（带 `id` 与
  `expand_web_url`）；**删除回 `data.image_data: null`**（没有可回显对象）
- 官方 UI 入口：面板内长按表情、长按评论里的附图 → 弹「添加到表情」；
  长按 ♡ 档里的表情 → 弹「删除表情」（删除还要二次确认「确定删除表情吗？」）
- **删除也是同一端点**（`collect_type=2`），没有独立删除接口
- 官方 ♡ 档的「上传自定义表情」= **上传（9.6.2）→ 收藏（9.6.3）**两步
- 重复收藏服务端会拒：实测回 `{"code":101066,"message":"已添加过该表情"}`；
  另一种形态是 `code=0` 但 `data.image_data` 为空（表情已在 ♡ 档里，
  服务端不回显）。两种都按「服务端拒绝」处理，错误原文透传给用户

#### 9.6.4 评论附图（`comment/add` 的 `image_data`）
评论发送（9.3，`data_type=4`/`commit_source=3`）在附图时 body 顶层多
`image_data`，表情类另带 `business_param.emoji_scene_type`：

```json
{
  "commit_source": 3, "group_id": "<vid>", "group_type": 30, "data_type": 4, "text": "",
  "image_data": [{
    "web_uri": "…", "width": 196, "height": 196, "format": "", "id": "6996174263220784397",
    "dynamic_url": "…", "image_type": 2,
    "thumb_nails": [{"web_uri": "", "width": 100, "height": 100, "format": "", "dynamic_url": "…"}],
    "image_name": ""
  }],
  "business_param": { …, "emoji_scene_type": 2 },
  "aid": 8662
}
```

| 场景 | `image_data` 条目 | `emoji_scene_type` |
| --- | --- | --- |
| 系统表情（😀 档） | **不带**（纯文本码 `[微笑]` 进 `text`，服务端无表情字段） | — |
| 第三方 GIF（GIF 档） | 表情对象全量（`id`/`dynamic_url`/`thumb_nails`/`image_name` 在场，`image_type=2`） | **2** |
| 自定义表情（♡ 档） | 表情对象全量（`image_type=1`，另带 `expand_web_url`） | **4** |
| 本地图片（输入框「图片」钮） | 只有 `web_uri`（= 9.6.2 的 `data`）+ 宽高 + `format` + `image_type=1` | **不带** |

- **本地图片是「发表时才上传」**：选图后**无任何请求**（实测），点发表才打
  9.6.2，再带 `image_data` 发评论
- 响应 `data.comment_info.common.content.image_data_list.image_data[]` 回显
  附图（另带 `expand_web_url` / `web_url` 缩略形态 `-image-v1:300:0.heic`）；
  纯图片评论的 `text` 为空串（`content_type=2`）

### 9.4.2 上报时机与历史页口径
样本 `captures/flows-20261025-coldstart.jsonl`（官方 App 冷启动 + 全播放流程）。
`read_history/update` + `read_progress/upload` 的触发点：

- **起播 5s 首报**：成对双接口，`current_play_position` 恰为 5000；
- **每 ~60s 播放时长**：仅 `read_progress/upload`（65s/126s 样本；暂停期间
  不计时——65→126→(185 被暂停打断)）；
- **暂停**（含开选集面板这类 UI 引发的暂停）：成对双接口，当前进度；
- **切集**：旧集成对上报收尾位置 + 新集 `read_progress/get`，新集开播后
  自己的 5s 里程碑再报；
- **关窗口不上报**（最后一条流量就是最后事件的上报，无 close flush）。

本项目口径（2026-10-18 定，两壳同规则：
`src/features/player/hooks/history-cadence.ts` ↔
`crates/hongguo-gpui/src/home/history_cadence.rs`）：**上报心跳与 5 秒落盘
节流解耦**，按播放位置判定节拍——新剧起播 5s 首报；**同剧切集立即报新集**
（历史行按剧聚合，切集后还显示上一集就是错的，比官方节奏激进一步，官方要
等新集自己的 5s 里程碑）；之后每 60s 一拍（暂停不计时）；回跳（以及 >60s
的前跳）立即补；暂停/切集/退出 force 收尾（仍守 <5 秒不建行的噪点门槛）。
上报落定后把 `watch-history` / `local-history` 查询标记过期但**不立刻重拉**
（`refetchType: 'none'`：首页「你的内容」种子吃同一份缓存，播放中重排会把
游标指到别的剧），历史页下次打开必拉新行。

> 为什么不能挂在落盘/帧事件上（2026-10-18 用户实测「第七集播完、第八集
> 开播，历史还是第七集；第八集十几秒才上报 5 秒」）：首报会被 5 秒落盘
> 窗口量化到 5–10 秒；而「每 60s 一拍」的计数只在真正上报时自增，恒定卡在
> 1，长集播放时云端进度整集停在首报值。

「历史」页筛选口径（抓包实测）：**已看完 = episode_cnt>0
且已看到最后一集**（`vid_index+1 >= episode_cnt`），与片内进度无关；
未看完取反；搜索为纯客户端标题过滤（placeholder：搜索看过的剧/收藏的剧/
点赞的剧/预约的剧）。

## 抓包数据文件
抓包工作流在仓库 `captures/`（gitignore，持久保留）：

- `addon.py`：mitmproxy dump 脚本（目标域名过滤，输出 JSONL，
  `MITM_OUT` 可覆盖输出路径）；`flows-YYYYMMDD.jsonl` 为按日抓包产物
  （请求头/参数/响应 body b64+brotli）。
- `audit.py`：参数对齐审计——新接口/改接口必须先跑它逐字段对齐：
  `python captures/audit.py [接口名过滤词]` 输出每个接口抓包在场的
  query/form body 字段/cookie 键/关键头。addon 存的 cookie 是逗号分隔
  （audit 已兼容）；`d_ticket` 是服务端历史下发凭据（本仓无来源），
  不算丢参。

1. `mitmdump -p 8080 -s captures/addon.py`（pip 装 mitmproxy ≥12；
   首次运行在 `~/.mitmproxy/` 生成 CA 并信任进系统——macOS 钥匙串 /
   Windows 证书存储）
2. 官方 App 走模拟器抓包：`adb push captures/hg-capture-apply.sh
   /data/local/tmp/` + `adb shell sh /data/local/tmp/hg-capture-apply.sh`
   应用 CA bind-mount + 封 QUIC（重启模拟器后重跑），再
   `adb shell settings put global http_proxy 10.0.2.2:8080` 指向 mitmdump
3. `python captures/ui.py dump|tap|shot` 驱动模拟器 UI（uiautomator；
   截图用 `adb pull` 落盘，**别用 PowerShell 重定向**——它会改写二进制）
4. 抓到的流量用 `python captures/flow.py <关键词> [--full]` 查（按端点/
   域名过滤，打印 query/body/响应）

⚠️ **模拟器 CA 装不上时的三个坑**（2026-10-25 实录，症状都是 mitmproxy
侧 `Client TLS handshake failed … does not trust the proxy's certificate`）：

1. **信任库真实路径是版本化 APEX**（`/apex/com.android.conscrypt@<版本>/cacerts`），
   未版本化的 `/apex/com.android.conscrypt/cacerts` 在本机镜像上是**空壳目录**，
   且 bind 到它会失败（脚本按通配符取真实目录）；
2. **CA 文件权限要 644 + root:root**——从 `/sdcard` 复制来的证书带
   media_rw 的 660 属主，App 进程读不到（日志里是
   `DirectoryCertificateSrc: … open failed: EACCES`）；
3. **SELinux 标签要 chcon 成 `system_security_cacerts_file`**——拷进
   `/data/local/tmp` 的默认标签是 `shell_data_file`，App 同样读不到。

