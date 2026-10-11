# hgplayer 1.1.8 逆向发现：接口参数与响应处理（REA/Ghidra）

来源：`红果短剧.exe`（hgplayer v1.1.8 Windows 版，SHA-256 `6c2d2aa0…bcfe9b`）。
该应用为 **Go (Wails v2) + WebView2**，Go 侧 API 层（`hg-go/pkg/hgapi`）带完整
中文 doc 注释编入二进制，Ghidra 反编译 + API 注册表（Name/Path/Method 三元组）
可逐函数锁定请求组装；本文只记录**反编译实证**的结论，未实测发包的项均标注。

## API 注册表（反编译直接读出 Name/Path/Method）

| Name                  | Path                                         | Method                                  |
| --------------------- | -------------------------------------------- | --------------------------------------- |
| `read_history_update` | `/reading/bookapi/read_history/update/v`     | POST                                    |
| `subscribe_delete`    | `/reading/user/subscribe/delete/v`           | POST                                    |
| `book_pack_fields`    | `/reading/distribution/book_pack_fields/v1/` | POST（`book_pack_channel="bookshelf"`） |

其余端点与 `docs/hongguo-api-endpoints.md` 台账一致（comment/del、do_action、
bookshelf/video/update、subscribe/list、landpage 等），不重复记录。

## 批量删除类（功能1 的依据）

### 历史删除 `DeleteHistory`（hg-go/pkg/hgapi/delete.go:23）

**复用 `read_history/update`**，body `{"update_datas": [ … ]}`，每条：
`{book_id:数字, book_type:2, vid:数字, vid_index, is_delete:true,
use_soft_delete:true, update_timestamp_ms:now, 其余进度字段全零/false}`
（与本项目 `report_watch_progress` 的 update_datas 完全同构，只翻两个布尔）。
响应校验 `data.update_fail_datas` 为空数组，非空 = 部分失败
（hgplayer 报「部分历史删除失败, 请稍后重试」）。`HistoryRef` 入参为
`{series_id, vid, vid_index}`，多条一次请求。

### 批量删预约 `DeleteReservations`（delete.go:102）

**`POST /reading/user/subscribe/delete/v`**，body
`{item_id:[数字…], all_select:bool, not_del_item_id:[…], is_online:bool, tab_type:13}`。
`item_id` 即 subscribe 列表条目的 series_id（与单条取消 uncover_subscribe 的
item_id 同源）；已上线/待上线按 tab 的 `is_online` 分请求。

### 批量取消收藏 `RemoveFavorites`

复用 `bookshelf/video/update`，**一个请求的 `update_bookshelf_video_list`
携带多条** `{book_id, book_type, modify_time, video_shelf_operate_type:1}`；
响应侧校验信封 `is_cancelled`。本项目 `collect_series_batch` 同构。

### 批量取消点赞 `RemoveLikes`

无批量端点——service 层**逐条**调 `do_action`（action_type=4 取消,
object_type=6）。前端循环 + 全量失效 interactState 是同构做法。

## 筛选类（功能2 的依据）

### 找剧「完结状态」（v1.1.7 新增）

- `SelectorPanel` 在服务端下发的筛选组后**追加一个客户端合成的
  FilterGroup**：`Key="creation_status"`，选项
  `已完结→id "creation_status_0"`、`连载中→id "creation_status_1"`。
- 选中值走 `Category`（landpage）请求体**顶层 `creation_status` 字段**
  （body map[string]string 取值原样透传），不进 select_items。
- 另有 `filterStatus`：对无状态字段的列表（如收藏列表只回 series_id）
  用 `book_pack_fields` 批量补字段后客户端过滤。

### 内容类型分类（seriesKind，rawSeries.model 内联）

- `content_type == 1` → 真人剧
- `content_type == 0x3ec (1004)` → 漫剧
- `video_category_type == "ai_video"`（8 字节比较）→ AI 剧
- AI 剧的 kind 表是**运行时学习**的（`learnKinds`/`resolveAIKinds` 带互斥
  锁缓存），非硬编码枚举。

本项目收藏页筛选按 `content_type` 1/1004 分档（`BookshelfEntry.contentType`
已有）；点赞列表（ugc/action/mget）条目无分类字段，未接类型筛选。

## 响应字段（功能5 的依据）

- 搜索结果的 `series_sub_title_list` **双形态**：JSON 字符串（内嵌数组）
  或数组。hgplayer `rawSeries.hotText`：遍历条目找含「热度」的那条作热度行
  （`stringslite_Index(s, "热度")`，6 字节）。本项目 `parse_search` 同语义归一
  为 `tags[] + heat_text`。
- 热度文本来自服务端原文（「4105万热度」），前端不做数字换算。
- 「新剧/爆剧/红果首发」标签在二进制中**不存在**（0 命中）——全部来自服务端
  字段（rec_tags/recommend_reason/badge 族），本项目 FeedItem/RankItem 已带。

## 其他注释佐证（对照台账无冲突）

- 点赞：`action_type 3 赞 4 取消, object_type=6`；评论/弹幕点赞 `8 赞 9 取消`
- 收藏：`video_shelf_operate_type 0 收 1 取消`
- 预约：`op_type 1 预约 2 取消`；预约列表 `is_online=true 已上线/false 待上线, tab_type=13`
- 追更列表：`action_channel=9, cursor_str 为上页返回的 cursor (JSON 字符串)`
- 搜索：`红果 7.3.9.32 只有 11=综合 1=小说 2=听书 三个分区`
- 进度查询：`read_progress/get body {book_ids:{2:[id...]}}`；
  `read_progress/list` 按内容类型批量、视频类回 series_id 维度
- 单剧详情：`series_id 逗号分隔可批量`（SeriesDetails）
- 新剧推荐：`cell_gender 2 全部 1 男 0 女`
- 短信验证码：`mobile 需 XOR(0x05) hex 编码`

## 未跟进项（记录备查）

- `book_pack_fields` 批量补详情：可为收藏/点赞页省掉逐条 resolve（本项目
  目前 useSeriesMeta 逐条），值得后续立项。
- 追更（subscribe 域 FollowUpdates/SetFollowUpdate）：本项目预约走日历域，
  两域不同源，未对齐。
- hgplayer 自有代理域 `hongguo.235698.xyz`（其更新/反馈服务），与红果官方
  接口无关。

## 参数规律总结与「免抓包推测」边界（2026-10-11 全族核对后沉淀）

### 通用参数及其特征

| 参数 | 特征 | 缺失/出错的症状 |
| ---- | ---- | ---- |
| `need_personal_recommend=1` | **cell/change 全族通用**（首页 feed tab_type=16、排行榜 26、新剧 limit=18 族）+ subscribe/list 日历形态；值恒 "1" | 不报错，但服务端回「非个性化混排」——条目集与第三方对不上（本次最大暗坑） |
| `session_id` | **服务端有状态游标**：首页响应下发 → 翻页每页回传；绝不能自造常量（命中旧快照）；同 session 重复请求会推进/冻结 | 缺失 → 个性化混排重排，相邻页重叠（实测 7-8/10 条重叠）；自造 → 读到陈旧快照 |
| `offset` / `subscribe_offset` | **按端点族二选一**：cell/change GET query 族只有 `offset`；subscribe/list 族必须 `offset`+`subscribe_offset`（同值）**双键齐传**，只传 offset 服务端静默无视 | 静默无视 = 同页死循环（实测 16 次同页收 422 条重复），无任何报错 |
| `limit` | 多为 `"0"`（页长服务端定，cell/change 实测 10-26 浮动）；显式值（18/20）服务端也收，next_offset 游标自适应 | —— |
| `target_date` | 只在切日期/翻页时在参，**首页（默认日）不带** | 带了=直查该日；不带=默认混排视图（两者条目集不同） |
| 筛选类（`creation_status`/`selected_items` 等） | **选中才在参，未选整个键省略**——不发空串/空数组（他家抓包实锤） | 发空串未实测出问题，但对齐起见省略 |
| `creation_status` 等客户端合成筛选 | 值为行 id（`creation_status_0` 已完结 / `_1` 连载中），走 body/query 顶层而非 select_items | —— |

### 三种翻页形态（按端点族）

| 族 | 请求 | 翻页键 |
| -- | ---- | ------ |
| cell/change GET 族（feed/排行/新剧/找剧 cell） | GET query | `offset` + `session_id`（首页不带） |
| subscribe/list 族（日历 tab_type=5 / 预约 13） | GET query | `offset` + `subscribe_offset`（同值）+ `session_id` 三键 |
| landpage POST 族（找剧） | POST body | body 内 `offset` + `session_id`（首页空串在参） |

### 下次能不能不抓包？——能推一半

**静态可推（Ghidra 三板斧，无需抓包）**：

1. **端点清单**：API 注册表是 `{Name, Path, Method}` 结构体数组，`dump_apistr.java`
   按 decomp 里的 `PTR_DAT_…` 地址直读——本会话 5 个端点全部这样坐实。
2. **请求键全集**：Go 函数体内的 `mapassign_faststr` 键名字符串（如
   `Calendar.c` 里的 target_date/offset/subscribe_offset/session_id）——
   键名与「哪些键存在」可 100% 静态读出。
3. **操作码/枚举值**：doc 注释编进二进制（action_type 3 赞 4 取消、
   video_shelf_operate_type 0/1、op_type 1/2 等），grep 即得。
4. **响应字段全集**：421 个 `json:"…"` tag 直读 + struct 类型名
   （rawHistory/rawSeries/…）。

**静态推不出、必须抓包验证的**：

1. **「在参与否」的动态细节**：静态只能看到「键会写进 map」，看不到
   「首页时这个键的值是空串还是整个不放」（本次 target_date 正是栽在这——
   静态读以为永远在参，抓包才发现首页不带）。
2. **值语义与组合效应**：`need_personal_recommend=1` 缺了不报错、只换数据集；
   `subscribe_offset` 与 `offset` 的配对关系——这些只有「对齐了参数还差什么
   就抓什么」才能发现。
3. **服务端有状态行为**：session 快照推进/冻结、静默忽略不认识的参数
   （不报错只给错数据，是最阴险的一类）。
4. **分页游标语义**：next_offset 步进 vs 实际条数（本次实测个性化混排下
   换页重排、重叠 8/10 是固有行为——客户端必须去重）。

**结论流程**：Ghidra 静态拿「端点 + 键全集 + 操作码」→ 直接写请求 →
按红线「请求+响应+UI 效果」三点一线验证 → **对不上再抓包对比**（抓包
聚焦值语义与组合，不必从头抓）。静态层能把抓包范围压缩到一次针对性
对比，但省不掉验证这一步。

—— 分析环境：Ghidra 12.1.4 headless（`tools/ghidra_projects/hgplayer` 项目，
480 个 hgapi/app/service 函数已反编译到 `Downloads/hg-extract/decomp/`）。
