# 红果官方 APK 端点全量清单（静态提取）

> 来源：官方 APK（com.phoenix.read）dex 字符串表静态提取，共 1302 条。
>
> ⚠️ **路径在场 ≠ 接口可用**：本清单只是官方客户端代码里出现过的 API 路径
> 字符串（按首段白名单过滤了文件系统路径；`/activity` 组含 H5 路由）。参数、
> 签名要求、响应形态一律以抓包三点一线为准——项目实际对接的口径见
> [`hongguo-api-endpoints.md`](hongguo-api-endpoints.md)。
>
> - 带 `POST`/`GET` 的 = 客户端 Retrofit 路径模板（`:xxx` 是路径参数）；无标记的 =
>   裸路径字符串（官方核心 API 走 TNet 编程式构建，不经 Retrofit 注解）。
> - 「在用」✅ = 本项目已接入的端点。
> - 合计 **1302 条**（其中带方法模板 884 条），项目在用 56 条。

## 汇总

| 业务域 | 条数 |
| --- | --- |
| 书城主站 bookapi（榜单/目录/搜索/分类/听书） | 225 |
| 阅读器内容 reader | 58 |
| 播放器 player（取流/分集/详情） | 13 |
| 评论 commentapi（弹幕/评论/剧评/回复） | 25 |
| 互动 articleapi（点赞等） | 25 |
| 剧评/话题/书单 ugc | 234 |
| 用户域 user | 188 |
| 消息 msgapi | 23 |
| 阅读域其它（搜索流/商业化/兑换） | 139 |
| 小说域其它 | 33 |
| 离线包 novel-dragon | 1 |
| 登录 passport | 47 |
| 任务激励 luckycat | 43 |
| 平台服务 service（设备注册/设置/日志/广告） | 10 |
| 广告 api | 61 |
| 抖音互通 aweme | 16 |
| 直播 webcast | 7 |
| 监控 monitor | 25 |
| 隐私合规 privacy | 7 |
| 用户中心 usercenter | 15 |
| 云推送 cloudpush | 14 |
| 活动 activity（H5 路由） | 11 |
| 支付网关 gateway/bytepay/cashier2/cashdesk | 45 |
| 其它（gkx 配置/video/praise/bookapi 裸路径等） | 37 |
| **合计** | **1302** |

## 书城主站 bookapi（榜单/目录/搜索/分类/听书）（225）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/reading/bookapi/all_items/v/` | — |  | classes24.dex |
| `/reading/bookapi/audio/batch_play/` | — |  | classes24.dex |
| `/reading/bookapi/audio/toneinfo` | — |  | classes24.dex |
| `/reading/bookapi/audio/toneinfo/` | — |  | classes24.dex |
| `/reading/bookapi/book_update/query/v/` | — |  | classes24.dex |
| `/reading/bookapi/bookmall/cell/change/v` | — | ✅ | classes15.dex, classes24.dex, classes3.dex, classes4.dex, classes5.dex, classes6.dex, classes9.dex |
| `/reading/bookapi/bookmall/tab/` | — | ✅ | classes5.dex |
| `/reading/bookapi/bookmall/tab/v` | — | ✅ | classes14.dex, classes15.dex, classes24.dex, classes3.dex, classes5.dex |
| `/reading/bookapi/bookmall/tab/v/` | — | ✅ | classes24.dex |
| `/reading/bookapi/bookshelf/push/bookinfo/v` | — |  | classes24.dex |
| `/reading/bookapi/directory/all_infos` | — |  | classes24.dex |
| `/reading/bookapi/directory/all_items` | — |  | classes24.dex |
| `/reading/bookapi/directory/all_items/v` | — |  | classes8.dex |
| `/reading/bookapi/directory/all_items/v:version/` | — |  | classes24.dex |
| `/reading/bookapi/new_category/front/v` | — |  | classes24.dex |
| `/reading/bookapi/new_category/front/v/` | — |  | classes24.dex |
| `/reading/bookapi/new_category/landing/v` | — |  | classes14.dex, classes15.dex, classes16.dex, classes24.dex |
| `/reading/bookapi/plan/v` | — | ✅ | classes14.dex, classes15.dex, classes16.dex, classes24.dex, classes4.dex, classes5.dex, classes6.dex, classes8.dex, classes9.dex |
| `/reading/bookapi/plan/v/` | — | ✅ | classes24.dex |
| `/reading/bookapi/plan/v:version/` | — | ✅ | classes5.dex |
| `/reading/bookapi/read_progress/recommend/` | — |  | classes24.dex |
| `/reading/bookapi/search/` | — |  | classes15.dex |
| `/reading/bookapi/search/cell/change/` | — |  | classes5.dex |
| `/reading/bookapi/search/cue/v` | — |  | classes14.dex, classes24.dex |
| `/reading/bookapi/search/suggest/v` | — | ✅ | classes14.dex, classes16.dex, classes24.dex |
| `/reading/bookapi/search/tab/v` | — | ✅ | classes14.dex, classes16.dex, classes24.dex, classes5.dex, classes6.dex |
| `/reading/bookapi/search/vision/v:version/` | — |  | classes14.dex, classes16.dex, classes3.dex, classes5.dex |
| `/reading/bookapi/surl/recommend/v` | — |  | classes24.dex |
| `/reading/bookapi/surl/v` | — |  | classes7.dex |
| `/reading/bookapi/surl/v:version/` | — |  | classes7.dex |
| `/reading/bookapi/upload/local_book/v:version/` | — |  | classes15.dex |
| `/reading/bookapi/video_tab/ab_result/v:version/` | — |  | classes24.dex |
| `/reading/bookapi/video_tab/continue_watch_ab/v/` | — |  | classes24.dex |
| `/reading/bookapi/widgets/book/` | — |  | classes24.dex |
| `/reading/bookapi/activate_plan/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/active_schema_report/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/activity/main_page/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/ai_dict/content_feedback/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/ai_dict/guess_your_query/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/ancient_books_read_progress/upload/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/audio/batch_play/` | POST |  | classes8.dex |
| `/reading/bookapi/audio/detail/` | GET |  | classes8.dex |
| `/reading/bookapi/audio/playurl/` | POST |  | classes8.dex |
| `/reading/bookapi/audio/toneinfo/` | GET |  | classes8.dex |
| `/reading/bookapi/audio_settings/load/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/audio_settings/upload/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/biz_static_config/get/v:version/` | GET |  | classes6.dex, classes8.dex |
| `/reading/bookapi/book_extra/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/book_update/query/v:version/` | GET |  | classes14.dex, classes8.dex |
| `/reading/bookapi/booklist/add/` | POST |  | classes8.dex |
| `/reading/bookapi/booklist/info/update/` | POST |  | classes8.dex |
| `/reading/bookapi/booklist/relation/update/` | POST |  | classes8.dex |
| `/reading/bookapi/bookmall/cell/change/v:version/` | GET | ✅ | classes8.dex |
| `/reading/bookapi/bookmall/homepage/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/bookmall/icon/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/bookmall/recommend_log_report/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/bookmall/tab/sort/v:version/` | POST | ✅ | classes8.dex |
| `/reading/bookapi/bookmall/tab/v` | GET | ✅ | classes7.dex |
| `/reading/bookapi/bookmall/tab/v:version/` | GET | ✅ | classes7.dex, classes8.dex |
| `/reading/bookapi/bookname/v:version/` | POST |  | classes14.dex, classes8.dex |
| `/reading/bookapi/bookshelf/add/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/bookshelf/banner/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/bookshelf/chase_book/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/bookshelf/check/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/bookshelf/commerce/banner/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/bookshelf/del/info/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/bookshelf/delete/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/bookshelf/info/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/bookshelf/list/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/bookshelf/local_book/list/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/bookshelf/local_book/update/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/bookshelf/panel_info/v:version/` | POST |  | classes14.dex, classes8.dex |
| `/reading/bookapi/bookshelf/push/bookinfo/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/bookshelf/recommend/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/bookshelf/snack_bar/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/bookshelf/tab/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/bookshelf/topping/get/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/bookshelf/topping/report/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/bookshelf/update/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/bookshelf/video/info/v:version/` | GET |  | classes6.dex, classes8.dex |
| `/reading/bookapi/bookshelf/video/list/v:version/` | GET | ✅ | classes8.dex |
| `/reading/bookapi/bookshelf/video/update/v:version/` | POST | ✅ | classes8.dex |
| `/reading/bookapi/bookstore/cell/change/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/bookstore/homepage/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/bookstore/landpage/` | GET |  | classes8.dex |
| `/reading/bookapi/category/booklist/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/category/list/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/category/newbooklist/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/category/newlist/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/category_daily_ranklist/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/chase_book/list/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/chase_book/update/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/client_report/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/coldstart/query_coldstart_type/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/coldstart/report/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/comic_tab/comic_detail/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/commerce/tag_info/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/common_ab_result/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/cue/fetch-debug/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/detail/rank/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/detail/related_book/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/detail/v:version/` | GET |  | classes14.dex, classes8.dex |
| `/reading/bookapi/detect/entity_in_image/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/directory/all_infos/v:version/` | GET |  | classes14.dex, classes8.dex |
| `/reading/bookapi/directory/all_items/v:version/` | GET |  | classes14.dex, classes8.dex |
| `/reading/bookapi/e_commerce/cart_num/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/e_commerce/shop_mall/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/excerpt/history/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/excerpt/list/v:version/` | GET |  | classes14.dex, classes8.dex |
| `/reading/bookapi/excerpt_page/v:version/` | GET |  | classes14.dex, classes8.dex |
| `/reading/bookapi/feedback/video_info/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/game_history/list/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/game_history/update/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/get_coldstart_data/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/good_comment/book/` | GET |  | classes8.dex |
| `/reading/bookapi/guide_info/get/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/huawei/v1/getBoardData/` | POST |  | classes8.dex |
| `/reading/bookapi/imc/resource/get/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/interact_novel/conf/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/interact_novel/list/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/interact_novel/multi_info/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/interactive/series/action/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/interactive/series/ad_strategy/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/interactive/series/backpack_panel/v:version/` | POST |  | classes15.dex, classes8.dex |
| `/reading/bookapi/interactive/series/point/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/interactive/series/progress/reset/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/interactive/series/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/item/content/unlock/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/last_page/detail/` | GET |  | classes8.dex |
| `/reading/bookapi/last_page/get_user_book_duration/v:version/` | POST |  | classes14.dex, classes8.dex |
| `/reading/bookapi/local_book/check` | GET |  | classes14.dex, classes8.dex |
| `/reading/bookapi/local_book/upload` | POST |  | classes14.dex, classes8.dex |
| `/reading/bookapi/lottery/detail/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/mall_benefit/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/msg_settings/load/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/msg_settings/upload/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/multi-detail/v:version/` | GET |  | classes14.dex, classes8.dex |
| `/reading/bookapi/multi_video_data/get/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/my_tab/plan/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/new_category/cell/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/new_category/check_manual_category/v:version/` | GET |  | classes16.dex, classes8.dex |
| `/reading/bookapi/new_category/front/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/new_category/landing/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/new_category/video/info/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/origin_activity/recommend/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/origin_activity/search_read_listen_checkin_book/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/original_fans/landpage/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/permission/read_end/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/plan/v:version/` | GET | ✅ | classes8.dex |
| `/reading/bookapi/preference_info/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/profit/making/strategy/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/publish/uncopyrighted_book/subscribe/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/ranklist/random/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/ranklist/sequence/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/read_encourage/get/v:version/` | GET |  | classes14.dex, classes8.dex |
| `/reading/bookapi/read_history/list/v:version/` | GET | ✅ | classes8.dex |
| `/reading/bookapi/read_history/update/v:version/` | POST | ✅ | classes8.dex |
| `/reading/bookapi/read_progress/get/v:version/` | POST |  | classes14.dex, classes8.dex |
| `/reading/bookapi/read_progress/item/sync/v:version/` | POST |  | classes14.dex, classes8.dex |
| `/reading/bookapi/read_progress/list/v:version/` | GET |  | classes14.dex, classes8.dex |
| `/reading/bookapi/read_progress/read_item_list/v:version/` | POST |  | classes14.dex, classes8.dex |
| `/reading/bookapi/read_progress/recent/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/read_progress/recommend/` | GET |  | classes5.dex, classes8.dex |
| `/reading/bookapi/read_progress/sync/v:version/` | POST |  | classes14.dex, classes8.dex |
| `/reading/bookapi/read_progress/upload/v:version/` | POST | ✅ | classes14.dex, classes8.dex |
| `/reading/bookapi/read_stat_detail/v:version/` | GET |  | classes14.dex, classes8.dex |
| `/reading/bookapi/reader_banner_resource/v:version/` | GET |  | classes24.dex, classes8.dex |
| `/reading/bookapi/reader_settings/load/v:version/` | GET |  | classes14.dex, classes8.dex |
| `/reading/bookapi/reader_settings/upload/v:version/` | POST |  | classes14.dex, classes8.dex |
| `/reading/bookapi/realtime/report/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/red_dot/get/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/refresh_reader_1col_strategy/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/research/get/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/resource/config/` | GET |  | classes8.dex |
| `/reading/bookapi/search/after_read/v:version/` | GET |  | classes23.dex, classes8.dex |
| `/reading/bookapi/search/baike_landing/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/search/cell/change/v:version/` | GET |  | classes16.dex, classes8.dex |
| `/reading/bookapi/search/cue/v:version/` | GET |  | classes14.dex, classes6.dex, classes8.dex |
| `/reading/bookapi/search/fetch-debug/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/search/full_content/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/search/hot-search/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/search/hot-tag/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/search/page/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/search/rank_list/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/search/reader_content/v:version/` | GET |  | classes14.dex, classes8.dex |
| `/reading/bookapi/search/reader_content/v:version/` | POST |  | classes14.dex, classes6.dex, classes8.dex |
| `/reading/bookapi/search/schema_landing/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/search/search/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/search/suggest/v:version/` | GET | ✅ | classes16.dex, classes8.dex |
| `/reading/bookapi/search/tab/v:version/` | GET | ✅ | classes8.dex |
| `/reading/bookapi/search/topic/suggest/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/search/uncover_subscribe/v:version/` | POST | ✅ | classes8.dex |
| `/reading/bookapi/search/user-info/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/search/vision/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/share/audio/detail/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/share/comic/detail/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/share/detail/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/spring_plan/` | GET |  | classes8.dex |
| `/reading/bookapi/sstimor/cell_info/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/sstimor/entry_info/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/sstimor/entry_type/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/sstimor/page/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/sstimor/quit/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/sstimor/tab/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/sstimor/time_info/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/surl/recommend/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/surl/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/surl/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/ug_check_ad/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/union_book/detail/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/upload/local_book/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/user/pay_detail/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/user/publish_vip_toast/v:version/` | GET |  | classes16.dex, classes8.dex |
| `/reading/bookapi/video/episode/add/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/video/episode/del/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/video/series/upsert/v:version/` | POST |  | classes8.dex |
| `/reading/bookapi/video/user/get/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/video_tab/ab_result/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/video_tab/continue_watch_ab/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/video_tab/multi_video_detail/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/video_tab/multi_video_model/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/video_tab/video_detail/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/video_tab/video_model/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/video_tab/video_recommend_book/v:version/` | GET |  | classes8.dex |
| `/reading/bookapi/widgets/book/` | GET |  | classes8.dex |

## 阅读器内容 reader（58）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/reading/reader/audio/playinfo` | — |  | classes24.dex |
| `/reading/reader/audio/timepoint` | — |  | classes24.dex |
| `/reading/reader/batch_full/v` | — |  | classes8.dex |
| `/reading/reader/book/lastpage/recommend/v` | — |  | classes24.dex |
| `/reading/reader/font/get/` | — |  | classes24.dex |
| `/reading/reader/full/v` | — |  | classes8.dex |
| `/reading/reader/recommend/book/` | — |  | classes24.dex |
| `/reading/reader/recommend/local_reader/v` | — |  | classes24.dex |
| `/reading/reader/recommend/lost_item/v` | — |  | classes24.dex |
| `/reading/reader/upload/picture/` | — |  | classes5.dex |
| `/reading/reader/annotation/get_bookcard/v:version/` | POST |  | classes14.dex, classes8.dex |
| `/reading/reader/annotation/query_dictionary/v:version/` | GET |  | classes14.dex, classes8.dex |
| `/reading/reader/audio/playinfo/` | GET |  | classes8.dex |
| `/reading/reader/audio/playinfo/` | POST |  | classes8.dex |
| `/reading/reader/audio/timepoint/` | GET |  | classes14.dex, classes6.dex, classes8.dex |
| `/reading/reader/banner/mget/` | GET |  | classes8.dex |
| `/reading/reader/batch_full/v:version/` | GET |  | classes14.dex, classes6.dex, classes8.dex |
| `/reading/reader/bonus/ad_reward/v:version/` | GET |  | classes8.dex |
| `/reading/reader/bonus/screen_close/v:version/` | GET |  | classes8.dex |
| `/reading/reader/bonus/v:version/` | GET |  | classes8.dex |
| `/reading/reader/book/chapter/correction/v:version/` | POST |  | classes14.dex, classes8.dex |
| `/reading/reader/book/getbookadvertisement/v:version/` | GET |  | classes8.dex |
| `/reading/reader/book/highlight/v:version/` | GET |  | classes14.dex, classes8.dex |
| `/reading/reader/book/lastpage/recommend/v:version/` | GET |  | classes8.dex |
| `/reading/reader/book/recommend_comic/v:version/` | GET |  | classes8.dex |
| `/reading/reader/book/recommend_data_plan/v:version/` | GET |  | classes8.dex |
| `/reading/reader/bookmark/add/` | POST |  | classes14.dex, classes8.dex |
| `/reading/reader/bookmark/del/` | POST |  | classes14.dex, classes8.dex |
| `/reading/reader/bookmark/del_book/` | POST |  | classes14.dex, classes8.dex |
| `/reading/reader/bookmark/list/` | GET |  | classes14.dex, classes8.dex |
| `/reading/reader/bookmark/personal_list/` | GET |  | classes14.dex, classes8.dex |
| `/reading/reader/bookmark/sync/` | POST |  | classes14.dex, classes8.dex |
| `/reading/reader/comic/conf/v:version/` | GET |  | classes8.dex |
| `/reading/reader/comic/item_tail/recommend/v:version/` | GET |  | classes8.dex |
| `/reading/reader/comic/last_page/recommend/v:version/` | GET |  | classes8.dex |
| `/reading/reader/dialog/addinto_bookshelf/v:version/` | GET |  | classes8.dex |
| `/reading/reader/feedback/typo/` | POST |  | classes14.dex, classes8.dex |
| `/reading/reader/font/get/` | GET |  | classes14.dex, classes8.dex |
| `/reading/reader/font/get_font_types/v:version/` | GET |  | classes14.dex, classes6.dex, classes8.dex |
| `/reading/reader/full/v:version/` | GET |  | classes14.dex, classes8.dex |
| `/reading/reader/item_summary/mget/v:version/` | GET |  | classes14.dex, classes8.dex |
| `/reading/reader/multi_full_image/v:version/` | GET |  | classes14.dex, classes6.dex, classes8.dex |
| `/reading/reader/newfull/v:version/` | GET |  | classes8.dex |
| `/reading/reader/recommend/book/` | GET |  | classes8.dex |
| `/reading/reader/recommend/item/` | GET |  | classes8.dex |
| `/reading/reader/recommend/local_reader/v:version/` | GET |  | classes8.dex |
| `/reading/reader/recommend/lost_item/v:version/` | GET |  | classes8.dex |
| `/reading/reader/research/check/` | GET |  | classes14.dex, classes8.dex |
| `/reading/reader/resource/trans_uri/v:version/` | POST |  | classes14.dex, classes8.dex |
| `/reading/reader/short_story/reader_type/` | GET |  | classes8.dex |
| `/reading/reader/strategy/v:version/` | GET |  | classes14.dex, classes8.dex |
| `/reading/reader/toast/get/` | GET |  | classes8.dex |
| `/reading/reader/transfer/` | GET |  | classes8.dex |
| `/reading/reader/ttsinfo/v:version/` | GET |  | classes8.dex |
| `/reading/reader/upload/picture/` | POST |  | classes8.dex |
| `/reading/reader/upload/secret_picture/` | POST |  | classes8.dex |
| `/reading/reader/video/timepoint/` | GET |  | classes8.dex |
| `/reading/reader/wish/v:version/` | POST |  | classes8.dex |

## 播放器 player（取流/分集/详情）（13）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/novel/player/album_detail` | — |  | classes24.dex |
| `/novel/player/multi_video_detail` | — | ✅ | classes24.dex |
| `/novel/player/multi_video_mode` | — |  | classes24.dex |
| `/novel/player/video_detail` | — | ✅ | classes24.dex |
| `/novel/player/video_model` | — |  | classes24.dex |
| `/novel/player/album_detail/v1/` | POST |  | classes14.dex, classes8.dex |
| `/novel/player/multi_video_detail/:dr_scene/v1/` | POST | ✅ | classes14.dex |
| `/novel/player/multi_video_detail/dynamic_data/v1/` | POST | ✅ | classes14.dex |
| `/novel/player/multi_video_detail/v1/` | POST | ✅ | classes14.dex |
| `/novel/player/multi_video_model/:dr_scene/v1/` | POST | ✅ | classes14.dex |
| `/novel/player/multi_video_model/v1/` | POST | ✅ | classes14.dex |
| `/novel/player/video_detail/v1/` | POST | ✅ | classes14.dex |
| `/novel/player/video_model/v1/` | POST |  | classes14.dex, classes8.dex |

## 评论 commentapi（弹幕/评论/剧评/回复）（25）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/novel/commentapi/` | — |  | classes14.dex |
| `/novel/commentapi/comment/do_action/v` | — | ✅ | classes4.dex |
| `/novel/commentapi/comment/list/` | — | ✅ | classes14.dex, classes3.dex, classes5.dex |
| `/novel/commentapi/comment/list/:group_id/v1/` | — | ✅ | classes24.dex |
| `/novel/commentapi/reply/list/` | — | ✅ | classes3.dex, classes5.dex |
| `/novel/commentapi/ai_generate/gen_setting/v1/` | POST |  | classes8.dex |
| `/novel/commentapi/ai_generate/v1/` | POST |  | classes8.dex |
| `/novel/commentapi/aigc/del/v1/` | POST |  | classes8.dex |
| `/novel/commentapi/aigc/user_list/:group_id/v1/` | POST |  | classes8.dex |
| `/novel/commentapi/comment/add/v1/` | POST | ✅ | classes8.dex |
| `/novel/commentapi/comment/del/v1/` | POST | ✅ | classes8.dex |
| `/novel/commentapi/comment/do_action/v1/` | POST | ✅ | classes8.dex |
| `/novel/commentapi/comment/list/:group_id/v1/` | POST | ✅ | classes8.dex |
| `/novel/commentapi/comment/report/v1/` | POST |  | classes8.dex |
| `/novel/commentapi/comment/self/:target_user_id/v1/` | POST |  | classes8.dex |
| `/novel/commentapi/comment/update/v1/` | POST |  | classes8.dex |
| `/novel/commentapi/edited_aigc/upload/v1/` | POST |  | classes8.dex |
| `/novel/commentapi/idea/list/:item_id/v1/` | POST |  | classes8.dex |
| `/novel/commentapi/relation/list/:target_user_id/v1/` | POST |  | classes8.dex |
| `/novel/commentapi/reply/add/v1/` | POST | ✅ | classes8.dex |
| `/novel/commentapi/reply/list/:comment_id/v1/` | POST | ✅ | classes8.dex |
| `/novel/commentapi/task/do/v1/` | POST |  | classes8.dex |
| `/novel/commentapi/user/profile/:target_user_id/v1/` | POST |  | classes8.dex |
| `/novel/commentapi/user_config/get/v1/` | POST |  | classes8.dex |
| `/novel/commentapi/user_config/set/v1/` | POST |  | classes8.dex |

## 互动 articleapi（点赞等）（25）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/novel/articleapi/do_action/v1/` | — | ✅ | classes24.dex |
| `/novel/articleapi/do_action/v:version/` | — | ✅ | classes24.dex |
| `/novel/articleapi/post_data/multi_get/v1/` | — |  | classes3.dex, classes4.dex, classes5.dex, classes9.dex |
| `/novel/articleapi/relation/mget_action_info/v1/` | — |  | classes24.dex |
| `/novel/articleapi/topic_tag/mix_list/v1/` | — |  | classes4.dex |
| `/novel/articleapi/article_list_statistic/get/v1/` | POST |  | classes8.dex |
| `/novel/articleapi/article_statistic_detail/get/v1/` | GET |  | classes8.dex |
| `/novel/articleapi/article_summary_statistic/get/v1/` | POST |  | classes8.dex |
| `/novel/articleapi/character/list/v1/` | POST |  | classes8.dex |
| `/novel/articleapi/del/v1/` | POST |  | classes8.dex |
| `/novel/articleapi/do_action/v1/` | POST | ✅ | classes8.dex |
| `/novel/articleapi/fans_club/join_panel/v1/` | GET |  | classes8.dex |
| `/novel/articleapi/feed/v1` | POST |  | classes8.dex |
| `/novel/articleapi/post/add/v1/` | POST |  | classes8.dex |
| `/novel/articleapi/post/update/v1/` | POST |  | classes8.dex |
| `/novel/articleapi/post_data/multi_get/v1/` | POST |  | classes8.dex |
| `/novel/articleapi/relation/get_user_action/v1/` | GET |  | classes8.dex |
| `/novel/articleapi/relation/mget_action_info/v1/` | POST |  | classes8.dex |
| `/novel/articleapi/report/v1/` | POST |  | classes8.dex |
| `/novel/articleapi/topic/add/v1/` | POST |  | classes8.dex |
| `/novel/articleapi/topic/multi_get/v1/` | POST |  | classes8.dex |
| `/novel/articleapi/topic/update/v1/` | POST |  | classes8.dex |
| `/novel/articleapi/topic_tag/mix_list/v1/` | POST |  | classes8.dex |
| `/novel/articleapi/video/add/v1/` | POST |  | classes8.dex |
| `/novel/articleapi/video/update/v1/` | POST |  | classes8.dex |

## 剧评/话题/书单 ugc（234）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/reading/ugc/ab/config/` | — |  | classes14.dex |
| `/reading/ugc/ab/config/v:version/` | — |  | classes24.dex |
| `/reading/ugc/action/mget/v` | — | ✅ | classes15.dex |
| `/reading/ugc/booklist/get/v` | — |  | classes15.dex |
| `/reading/ugc/editor/recommend/v` | — |  | classes14.dex |
| `/reading/ugc/fan_rank/list/v` | — |  | classes9.dex |
| `/reading/ugc/feed/cell_view/v` | — |  | classes9.dex |
| `/reading/ugc/feed/inside_content/v` | — |  | classes9.dex |
| `/reading/ugc/feed/inside_content/v:version/` | — |  | classes6.dex |
| `/reading/ugc/follow/get_unread_count/v/` | — |  | classes24.dex |
| `/reading/ugc/forum/desc/v:version/` | — |  | classes5.dex |
| `/reading/ugc/forum/detail/v` | — |  | classes9.dex |
| `/reading/ugc/im/desc/get/v` | — |  | classes9.dex |
| `/reading/ugc/im/robot_list/get/v` | — |  | classes9.dex |
| `/reading/ugc/item/mix_data/get` | — |  | classes14.dex |
| `/reading/ugc/manager_comment/unread_count` | — |  | classes24.dex |
| `/reading/ugc/message/topic_reply/v` | — |  | classes9.dex |
| `/reading/ugc/person/get_mixed/v:version/` | — |  | classes3.dex |
| `/reading/ugc/person/product/v` | — |  | classes9.dex |
| `/reading/ugc/post_event/v` | — |  | classes24.dex |
| `/reading/ugc/postdata/comment/v` | — |  | classes9.dex |
| `/reading/ugc/postdata/detail/v` | — |  | classes9.dex |
| `/reading/ugc/postdata/detail/v:version/` | — |  | classes6.dex |
| `/reading/ugc/postdata/message/v` | — |  | classes9.dex |
| `/reading/ugc/privacy/get/v` | — |  | classes9.dex |
| `/reading/ugc/reply/topic/v` | — |  | classes9.dex |
| `/reading/ugc/topic/content/v:version/` | — |  | classes5.dex |
| `/reading/ugc/topic/desc/v` | — |  | classes9.dex |
| `/reading/ugc/topic/get_by_recommend/v:version/` | — |  | classes24.dex |
| `/reading/ugc/topic/get_read_history/v` | — |  | classes9.dex |
| `/reading/ugc/topic/get_read_history/v:version/` | — |  | classes5.dex |
| `/reading/ugc/topic/get_recommend_by_tags/v` | — |  | classes24.dex |
| `/reading/ugc/topic/get_unread_count/v:version/` | — |  | classes24.dex |
| `/reading/ugc/topic_tag/get/v` | — |  | classes24.dex |
| `/reading/ugc/ab/config/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/ab/config/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/action/get/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/action/mget/v:version/` | GET | ✅ | classes6.dex, classes8.dex |
| `/reading/ugc/action/set/v:version/` | POST |  | classes3.dex, classes8.dex |
| `/reading/ugc/action/user/get/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/activity/card/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/activity/get_award/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/activity/get_rank/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/activity/outshow_card/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/activity/task_award/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/activity/tasks/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/admin/apply/v:version` | POST |  | classes8.dex |
| `/reading/ugc/admin/apply_qualification/query/v:version` | GET |  | classes8.dex |
| `/reading/ugc/admin/exec_permission/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/admin/get_permission/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/admin/get_target/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/annual_report/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/audio/item/comment/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/audio/item/danmaku/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/author/topic/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/author/update/info/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/battery/rank/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/battery/status/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/book/circle/auth/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/book/circle/comment/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/book/circle/topic/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/book_end/entrance/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/book_quote/search/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/booklist/get/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/booklist/read_history/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/comic/item/comment/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/comment/emoticon/collect/v:version/` | POST |  | classes8.dex, classes9.dex |
| `/reading/ugc/comment/emoticon/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/data/del/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/dy/access_token/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/dy/jsb_sig/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/dy/proxy/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/editor/recommend/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/editor_config/get/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/fan_rank/list/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/fan_rank/tab/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/fan_rank/user/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/feed/cell_data/v:version/` | GET |  | classes8.dex, classes9.dex |
| `/reading/ugc/feed/cell_view/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/feed/inside_content/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/feed/tab_style/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/follow/get_unread_count/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/follow/get_user_relation/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/forum/banner/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/forum/desc/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/forum/detail/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/forum/get_mixed/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/forum/square/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/forum/square_cell/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/forum/top_list/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/general/follow/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/general/multi_follow/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/gold_coin/bonus_status/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/gold_coin/receive_coin/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/gold_coin/task/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/idea/comment_list/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/idea/list/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/im/conversation/destroy/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/im/conversation/join/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/im/conversation_config/get/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/im/conversation_config/set/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/im/conversation_info/get/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/im/conversation_info/set/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/im/conversations/get/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/im/desc/get/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/im/fresh_item/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/im/gen_info/get/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/im/invite/candidate/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/im/invite/send/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/im/mixed_robot_list/get/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/im/robot/feedback/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/im/robot/manage/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/im/robot_follow_up/get/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/im/robot_info/get/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/im/robot_list/get/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/im/robot_mixed_data/get/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/im/robot_script/get/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/im/robot_setting/set/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/im/robot_tts/get/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/im/user/remove/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/im/user/search/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/im/user_info/get/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/item/author/speak/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/item/bullet/comment/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/item/clock-in/info/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/item/clock-in/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/item/mix_data/get/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/manager_comment/get_comments/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/manager_comment/unread/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/manager_comment/unread_count/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/menu/detail/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/message/book_comment_reply/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/message/book_reply_reply/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/message/item_reply/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/message/post_comment_reply/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/message/topic_detail/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/message/topic_reply/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/multi_invite/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/novel_comment/add/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/novel_comment/book/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/novel_comment/book_detail/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/novel_comment/del/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/novel_comment/digg/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/novel_comment/item/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/novel_comment/rank_book/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/novel_comment/report/v:version/` | POST |  | classes3.dex, classes8.dex |
| `/reading/ugc/novel_comment/topic/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/novel_comment/update/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/person/bookshelf/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/person/get_mixed/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/person/postdata/get/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/person/product/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/person_comment/book/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/person_comment/item/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/person_comment/list/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/post_event/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/postdata/add/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/postdata/comment/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/postdata/del/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/postdata/detail/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/postdata/list/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/postdata/message/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/postdata/modify/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/postdata/unblock/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/praise/get_message/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/praise/list/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/praise/motivate/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/praise/rank/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/praise/rank_record/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/praise/status/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/praise/user/record/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/privacy/get/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/privacy/set/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/produce/interaction/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/produce/task/detail/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/produce/task_reward/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/produce/task_reward_rank/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/produce/task_reward_stat/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/produce/task_reward_stat_list/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/produce/tasks/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/produce/tasktab/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/rank_book/get/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/reply/add/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/reply/book_detail/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/reply/del/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/reply/item_detail/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/reply/topic/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/search/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/song/get_by_collection_id/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/song/get_collection/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/song/search/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/sticker/get/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/sticker/user_sticker/update/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/strategy/reader/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/strategy/set/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/tag/get_mixed/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/tag/option/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/tag/option/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/tag_info/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/template/get_mixed/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/template/list/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/topic/add/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/topic/content/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/topic/delete/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/topic/delete_read_history/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/topic/desc/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/topic/get_by_recommend/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/topic/get_by_tag/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/topic/get_read_history/v:version/` | GET |  | classes5.dex, classes8.dex |
| `/reading/ugc/topic/get_recommend_by_tags/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/topic/get_unread_count/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/topic/get_unread_info/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/topic/get_user_topic/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/topic/hot_book/comments/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/topic/list/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/topic/modify/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/topic/read_history/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/topic/savior_rank/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/topic_forum/invite_by_recommend/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/topic_tag/get/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/topic_tag/list/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/topic_tag/search/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/ugc_report/get/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/ugc_report/get_list/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/urge_update/add/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/urge_update/get/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/user/get_book_rec_history/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/user/get_recommend/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/video/token/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/video/upload/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/vote/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/wiki/get/v:version/` | GET |  | classes8.dex |
| `/reading/ugc/wordlink/add/v:version/` | POST |  | classes8.dex |
| `/reading/ugc/writer_center/unread_count/v:version/` | GET |  | classes8.dex |

## 用户域 user（188）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/reading/user/activate/v` | — |  | classes24.dex |
| `/reading/user/author_book/get/v` | — |  | classes9.dex |
| `/reading/user/basic_info/get/v` | — |  | classes9.dex |
| `/reading/user/basic_info/get/v:version/` | — |  | classes5.dex |
| `/reading/user/browsing_history/tab` | — |  | classes24.dex |
| `/reading/user/event/report` | — |  | classes15.dex |
| `/reading/user/info/init` | — | ✅ | classes24.dex |
| `/reading/user/listen/pre_unlock/task_panel/v` | — |  | classes2.dex |
| `/reading/user/preference/info/` | — |  | classes16.dex |
| `/reading/user/share/info/v` | — |  | classes24.dex |
| `/reading/user/ab/data/` | GET |  | classes8.dex |
| `/reading/user/ab/result/` | POST |  | classes8.dex |
| `/reading/user/activate/v:version/` | GET |  | classes8.dex |
| `/reading/user/activity/action` | POST |  | classes8.dex |
| `/reading/user/activity/info` | GET |  | classes8.dex |
| `/reading/user/ad_free/popup/v:version/` | GET |  | classes8.dex |
| `/reading/user/ad_free/pre_save/panel/v:version/` | GET |  | classes8.dex |
| `/reading/user/appeal/submit/v:version/` | POST |  | classes8.dex |
| `/reading/user/apply/list/v:version/` | POST |  | classes8.dex |
| `/reading/user/apply/member_type/v:version/` | POST |  | classes8.dex |
| `/reading/user/author/commit_info` | POST |  | classes8.dex |
| `/reading/user/author/get_book` | GET |  | classes8.dex |
| `/reading/user/author/get_verify_info` | GET |  | classes8.dex |
| `/reading/user/author/register/v:version/` | POST |  | classes8.dex |
| `/reading/user/author_book/get/v:version/` | GET |  | classes8.dex |
| `/reading/user/authorization/info/update/v:version/` | POST |  | classes8.dex |
| `/reading/user/authorization/info/v:version/` | GET |  | classes8.dex |
| `/reading/user/authorize/get_token/v:version/` | GET |  | classes8.dex |
| `/reading/user/authorize/verify_token/v:version/` | POST |  | classes8.dex |
| `/reading/user/auto_reading/pre_save/panel/v:version/` | GET |  | classes8.dex |
| `/reading/user/ban_appeal_info/get/v:version/` | POST |  | classes8.dex |
| `/reading/user/basic_info/get/v:version/` | GET |  | classes8.dex |
| `/reading/user/battery/charge/v:version/` | POST |  | classes8.dex |
| `/reading/user/battery/lotteries/detail/v:version/` | GET |  | classes8.dex |
| `/reading/user/bc/account/binding/info/v:version/` | GET |  | classes8.dex |
| `/reading/user/bc/account/binding/update/v:version/` | POST |  | classes8.dex |
| `/reading/user/bind/relate_work/v:version/` | POST |  | classes8.dex |
| `/reading/user/bind/v:version/` | POST |  | classes8.dex |
| `/reading/user/book/paid/record/v:version/` | GET |  | classes8.dex |
| `/reading/user/book/purchase/v:version/` | POST |  | classes8.dex |
| `/reading/user/browsing_history/tab` | GET |  | classes8.dex |
| `/reading/user/card/ocr` | POST |  | classes8.dex |
| `/reading/user/celebrity/homepage/v:version/` | GET |  | classes8.dex |
| `/reading/user/celebrity/list/v:version/` | GET |  | classes8.dex |
| `/reading/user/celebrity/works/v:version/` | GET |  | classes8.dex |
| `/reading/user/cert/authorize/v:version` | POST |  | classes8.dex |
| `/reading/user/cert/qualification/query/v:version` | GET |  | classes8.dex |
| `/reading/user/cert/two_element/v:version` | POST |  | classes8.dex |
| `/reading/user/chargeable_book/recommend/v:version/` | GET |  | classes8.dex |
| `/reading/user/common/motivate_strategy/v:version/` | GET |  | classes8.dex |
| `/reading/user/contract/create` | POST |  | classes8.dex |
| `/reading/user/contract/finish` | POST |  | classes8.dex |
| `/reading/user/contract/get` | GET |  | classes8.dex |
| `/reading/user/contract/list` | GET |  | classes8.dex |
| `/reading/user/contract/user/check` | GET |  | classes8.dex |
| `/reading/user/creator/center/detail/v:version/` | GET |  | classes6.dex, classes8.dex |
| `/reading/user/creator/data/content_stat_detail/v:version/` | GET |  | classes8.dex |
| `/reading/user/creator/data/content_tab/v:version/` | GET |  | classes6.dex, classes8.dex |
| `/reading/user/creator/data/list/v:version/` | GET |  | classes8.dex |
| `/reading/user/credit_granted/entry/v:version/` | GET |  | classes8.dex |
| `/reading/user/debug/preference/info/` | GET |  | classes8.dex |
| `/reading/user/debug/preference/set/` | POST |  | classes8.dex |
| `/reading/user/decoration/list/v:version/` | GET |  | classes8.dex |
| `/reading/user/event/report` | POST |  | classes8.dex |
| `/reading/user/follow/more_users/v:version/` | GET |  | classes6.dex, classes8.dex |
| `/reading/user/force_update/window_info` | GET |  | classes8.dex |
| `/reading/user/get_recommend_userinfo` | GET |  | classes8.dex |
| `/reading/user/identity/auth` | POST |  | classes8.dex |
| `/reading/user/info/init` | POST | ✅ | classes8.dex |
| `/reading/user/info/report/v:version/` | POST | ✅ | classes8.dex |
| `/reading/user/info/sync` | POST | ✅ | classes8.dex |
| `/reading/user/info/v:version/` | GET | ✅ | classes8.dex |
| `/reading/user/listen/detail/page/v:version/` | GET |  | classes8.dex |
| `/reading/user/listen/meal/page/v:version/` | GET |  | classes8.dex |
| `/reading/user/listen/motivate_strategy/v:version/` | GET |  | classes8.dex |
| `/reading/user/listen/pop_style/v:version/` | GET |  | classes8.dex |
| `/reading/user/listen/pre_unlock/task/v:version/` | GET |  | classes8.dex |
| `/reading/user/listen/pre_unlock/task_panel/v:version/` | GET |  | classes8.dex |
| `/reading/user/listen/welfare_rain/detail/v:version/` | GET |  | classes8.dex |
| `/reading/user/multi_register/v:version/` | POST |  | classes8.dex |
| `/reading/user/multi_share_info/v:version/` | POST |  | classes6.dex, classes8.dex |
| `/reading/user/night_mode/preference/info/` | GET |  | classes8.dex |
| `/reading/user/night_mode/preference/set/` | POST |  | classes8.dex |
| `/reading/user/operation_permission/get/v:version/` | POST |  | classes8.dex |
| `/reading/user/pack/user_data/` | POST |  | classes8.dex |
| `/reading/user/praise/ad_callback/v:version/` | GET |  | classes8.dex |
| `/reading/user/praise/callback/v:version/` | POST |  | classes8.dex |
| `/reading/user/praise/callback_v2/v:version/` | POST |  | classes8.dex |
| `/reading/user/praise/cancel/v:version/` | POST |  | classes8.dex |
| `/reading/user/praise/create_order/v:version/` | POST |  | classes8.dex |
| `/reading/user/praise/flower_entry/v:version/` | GET |  | classes8.dex |
| `/reading/user/praise/product/info/v:version/` | GET |  | classes8.dex |
| `/reading/user/praise/ugc_switch_info/v:version/` | GET |  | classes8.dex |
| `/reading/user/praise/ugc_switch_open/v:version/` | POST |  | classes8.dex |
| `/reading/user/preference/info/` | GET |  | classes8.dex |
| `/reading/user/preference/set/` | POST |  | classes8.dex |
| `/reading/user/preference_settings/get_labels/` | GET |  | classes8.dex |
| `/reading/user/preference_settings/set_profile/v:version/` | POST |  | classes8.dex |
| `/reading/user/prepare/comment/book_list/v:version/` | GET |  | classes8.dex |
| `/reading/user/privilege/active/v:version/` | POST |  | classes8.dex |
| `/reading/user/privilege/add/v:version/` | POST |  | classes8.dex |
| `/reading/user/privilege/auth_douyin/v:version/` | GET |  | classes8.dex |
| `/reading/user/privilege/consume/v:version/` | POST |  | classes8.dex |
| `/reading/user/privilege/login/popup/v:version/` | GET |  | classes8.dex |
| `/reading/user/privilege/recall/v:version/` | POST |  | classes8.dex |
| `/reading/user/profile/get/v:version/` | GET |  | classes8.dex |
| `/reading/user/profile/golden_line_item/info/v:version/` | GET |  | classes8.dex |
| `/reading/user/question/answer/v:version/` | POST |  | classes8.dex |
| `/reading/user/read_card/detail/v:version/` | GET |  | classes8.dex |
| `/reading/user/read_card/purchase/v:version/` | POST |  | classes16.dex, classes8.dex |
| `/reading/user/read_card/task_detail/v:version/` | GET |  | classes8.dex |
| `/reading/user/read_profile/page/v:version/` | GET |  | classes8.dex |
| `/reading/user/reading_day/land_page/v:version/` | POST |  | classes8.dex |
| `/reading/user/recommend_feature/report/v:version/` | POST |  | classes8.dex |
| `/reading/user/red_packet/receive/v:version/` | POST |  | classes8.dex |
| `/reading/user/redeem_code/use/` | POST |  | classes8.dex |
| `/reading/user/register/info/v:version/` | GET |  | classes8.dex |
| `/reading/user/register/multi_info/v:version/` | GET |  | classes8.dex |
| `/reading/user/register/quick/v:version/` | POST |  | classes6.dex, classes8.dex |
| `/reading/user/register/v:version/` | POST |  | classes8.dex |
| `/reading/user/remind/op/v:version/` | POST |  | classes8.dex |
| `/reading/user/search/v:version/` | GET |  | classes8.dex |
| `/reading/user/sensitive_type/v:version/` | GET |  | classes8.dex |
| `/reading/user/set/userattr/v:version/` | POST |  | classes8.dex |
| `/reading/user/share/channel_info/v:version/` | GET |  | classes6.dex, classes8.dex |
| `/reading/user/share/check_spam/v:version/` | POST |  | classes24.dex, classes8.dex |
| `/reading/user/share/common_conf/v:version/` | GET |  | classes8.dex |
| `/reading/user/share/entrance_info/` | GET |  | classes8.dex |
| `/reading/user/share/hot_list/v:version/` | POST |  | classes8.dex |
| `/reading/user/share/info/v:version/` | GET |  | classes8.dex |
| `/reading/user/share/link_card_callback/v:version/` | GET |  | classes8.dex |
| `/reading/user/share/live_room/v:version/` | POST |  | classes8.dex |
| `/reading/user/share/paragraph/v:version/` | POST |  | classes8.dex |
| `/reading/user/share/poster/v:version/` | GET |  | classes8.dex |
| `/reading/user/share/short_url/` | GET |  | classes6.dex, classes8.dex |
| `/reading/user/share/token/` | GET |  | classes8.dex |
| `/reading/user/shop_mall/user_info/v:version/` | GET |  | classes8.dex |
| `/reading/user/shopping_mall/config/v:version/` | GET |  | classes8.dex |
| `/reading/user/sign/customer` | POST |  | classes8.dex |
| `/reading/user/sms/code/send` | POST |  | classes8.dex |
| `/reading/user/sstimor_box_info/v:version/` | GET |  | classes8.dex |
| `/reading/user/subscribe/delete/v:version/` | POST |  | classes8.dex |
| `/reading/user/subscribe/list/v:version/` | GET | ✅ | classes8.dex |
| `/reading/user/switch/info` | GET |  | classes8.dex |
| `/reading/user/switch/update` | POST |  | classes8.dex |
| `/reading/user/task/finish/v:version/` | POST |  | classes8.dex |
| `/reading/user/task/is_open/v:version/` | GET |  | classes8.dex |
| `/reading/user/union_vip/entrance/v:version/` | GET |  | classes8.dex |
| `/reading/user/union_vip/info/v:version/` | POST |  | classes8.dex |
| `/reading/user/video/paid/record/v:version/` | GET |  | classes8.dex |
| `/reading/user/vip/callback/v:version/` | POST |  | classes8.dex |
| `/reading/user/vip/callback_v2/v:version/` | POST |  | classes8.dex |
| `/reading/user/vip/cancel_order/v:version/` | POST |  | classes8.dex |
| `/reading/user/vip/create_order/v:version/` | POST |  | classes8.dex |
| `/reading/user/vip/create_union/v:version/` | POST |  | classes8.dex |
| `/reading/user/vip/discount/show_info/v:version/` | GET |  | classes8.dex |
| `/reading/user/vip/entrance/info/v:version/` | GET |  | classes8.dex |
| `/reading/user/vip/high_value_user/ab_result/v:version/` | GET |  | classes8.dex |
| `/reading/user/vip/inspire/show_info/v:version/` | GET |  | classes8.dex |
| `/reading/user/vip/multi_privilege/info/v:version/` | POST |  | classes8.dex |
| `/reading/user/vip/multi_product/info/v:version/` | POST |  | classes8.dex |
| `/reading/user/vip/order_list/v:version/` | GET |  | classes8.dex |
| `/reading/user/vip/order_query/info/v:version/` | GET |  | classes8.dex |
| `/reading/user/vip/order_status/v:version/` | GET |  | classes8.dex |
| `/reading/user/vip/privilege/batch_preload/v:version/` | GET |  | classes8.dex |
| `/reading/user/vip/privilege/info/v:version/` | GET |  | classes8.dex |
| `/reading/user/vip/product/info/v:version/` | GET |  | classes8.dex |
| `/reading/user/vip/promotion/show_list/v:version/` | GET |  | classes8.dex |
| `/reading/user/vip/promotion/strategy_list/v:version/` | GET |  | classes8.dex |
| `/reading/user/vip/promotion/strategy_list/v:version/` | POST |  | classes8.dex |
| `/reading/user/vip/reconcile_trigger/v:version/` | POST |  | classes8.dex |
| `/reading/user/vip/renew/info_list/v:version/` | GET |  | classes8.dex |
| `/reading/user/vip/subscribe_callback/v:version/` | POST |  | classes8.dex |
| `/reading/user/vip/subscribe_pay_callback/v:version/` | POST |  | classes8.dex |
| `/reading/user/vip/task/detail/v:version/` | GET |  | classes8.dex |
| `/reading/user/vip/task/finish/v:version/` | POST |  | classes8.dex |
| `/reading/user/vip/trigger_callback/v:version/` | POST |  | classes8.dex |
| `/reading/user/wallet/account/info/v:version/` | GET |  | classes8.dex |
| `/reading/user/wallet/recharge/callback/v:version/` | POST |  | classes8.dex |
| `/reading/user/wallet/recharge/callback_v2/v:version/` | POST |  | classes8.dex |
| `/reading/user/wallet/recharge/create_order/v:version/` | POST |  | classes8.dex |
| `/reading/user/wallet/recharge/product/v:version/` | GET |  | classes8.dex |
| `/reading/user/wallet/recharge/record/v:version/` | GET |  | classes8.dex |
| `/reading/user/widgets/count` | GET |  | classes8.dex |
| `/reading/user/withdraw/callback/v:version/` | POST |  | classes8.dex |
| `/reading/user/withdraw/create_order/v:version/` | POST |  | classes8.dex |
| `/reading/user/withdraw/withdraw_info/v:version/` | GET |  | classes8.dex |
| `/reading/user/worn/decoration/v:version/` | POST |  | classes8.dex |

## 消息 msgapi（23）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/reading/msgapi/getnewmsggroupcount/` | — |  | classes14.dex |
| `/reading/msgapi/getnewmsggroupcount/v:version/` | — |  | classes24.dex |
| `/reading/msgapi/syncAll/v` | — |  | classes24.dex |
| `/reading/msgapi/syncAll/v:version/` | — |  | classes24.dex |
| `/reading/msgapi/clear_unread_messages/v:version/` | POST |  | classes8.dex |
| `/reading/msgapi/delmsg/v:version/` | POST |  | classes8.dex |
| `/reading/msgapi/get_inner_push_msg/v:version/` | POST |  | classes6.dex, classes8.dex |
| `/reading/msgapi/get_red_dot_notify/v:version/` | GET |  | classes8.dex |
| `/reading/msgapi/get_ugc_message/v:version/` | GET |  | classes8.dex |
| `/reading/msgapi/getinteractivation/v:version/` | GET |  | classes8.dex |
| `/reading/msgapi/getmessagebydirection/v:version/` | GET |  | classes8.dex |
| `/reading/msgapi/getnewmsgcount/v:version/` | POST |  | classes8.dex |
| `/reading/msgapi/getnewmsggroupcount/v:version/` | GET |  | classes6.dex, classes8.dex |
| `/reading/msgapi/getnotification/v:version/` | GET |  | classes8.dex |
| `/reading/msgapi/getrelatedynamic/v:version/` | GET |  | classes8.dex |
| `/reading/msgapi/getrelatedynamiccount/v:version/` | GET |  | classes8.dex |
| `/reading/msgapi/inner_push_msg_ack/v:version/` | POST |  | classes6.dex, classes8.dex |
| `/reading/msgapi/msg_group_list/v:version/` | GET |  | classes8.dex |
| `/reading/msgapi/multi_tab_unread_count/v:version/` | GET |  | classes8.dex |
| `/reading/msgapi/notifylist/v:version/` | GET |  | classes8.dex |
| `/reading/msgapi/red_dot/query/v:version/` | POST |  | classes8.dex |
| `/reading/msgapi/red_dot/report/v:version/` | POST |  | classes8.dex |
| `/reading/msgapi/syncAll/v:version/` | POST |  | classes8.dex |

## 阅读域其它（搜索流/商业化/兑换）（139）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/reading/ai_search_stream/common/v` | — |  | classes24.dex |
| `/reading/ai_search_stream/common/v:version/` | — |  | classes3.dex |
| `/reading/ai_search_stream/im_chat_event/v:version/` | — |  | classes2.dex |
| `/reading/bookmall_stream/tab/v:version/` | — |  | classes15.dex, classes24.dex, classes5.dex |
| `/reading/commerceapi/product/card/v1/` | — |  | classes24.dex |
| `/reading/crypt/registerkey` | — |  | classes24.dex |
| `/reading/search_stream/tag/v1/` | — |  | classes16.dex |
| `/reading/search_stream/vision/v1/` | — |  | classes16.dex |
| `/reading/surl_bookmall_stream` | — |  | classes7.dex |
| `/reading/surl_bookmall_stream/v:version/` | — |  | classes24.dex, classes7.dex |
| `/reading/activity/list/v:version` | GET |  | classes5.dex |
| `/reading/ai_search_stream/clear_messages/v:version/` | POST |  | classes8.dex |
| `/reading/ai_search_stream/conversation_info/v:version/` | POST |  | classes8.dex |
| `/reading/ai_search_stream/delete_chat_message_range/v:version/` | POST |  | classes8.dex |
| `/reading/ai_search_stream/get_latest_messages/v:version/` | POST |  | classes8.dex |
| `/reading/ai_search_stream/load_more_messages/v:version/` | POST |  | classes8.dex |
| `/reading/ai_search_stream/post_chat_event/v:version/` | POST |  | classes8.dex |
| `/reading/ai_search_stream/update_chat_message_feedback/v:version/` | POST |  | classes8.dex |
| `/reading/commerceapi/ab/config/v1/` | POST |  | classes8.dex |
| `/reading/commerceapi/ad/audio/player/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/ad/auth_benefit/apply/v1/` | POST |  | classes8.dex |
| `/reading/commerceapi/ad/auth_benefit/consult/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/ad/book/lastpage/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/ad/comment/v1/` | POST |  | classes8.dex |
| `/reading/commerceapi/ad/delivery_plan/v:version/` | GET |  | classes8.dex |
| `/reading/commerceapi/ad/feature/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/ad/mix_game/v1/` | POST |  | classes8.dex |
| `/reading/commerceapi/ad/patch_plan/v:version/` | GET |  | classes8.dex |
| `/reading/commerceapi/ad/remain/landing_schema/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/ad/report/v:version/` | POST |  | classes24.dex, classes8.dex |
| `/reading/commerceapi/ad/ug/v1/` | POST |  | classes8.dex |
| `/reading/commerceapi/annual_ecom_summary/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/apply/benefit/v1/` | POST |  | classes8.dex |
| `/reading/commerceapi/apply_spring_festival_coupon/v1/` | POST |  | classes8.dex |
| `/reading/commerceapi/book/product/detail/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/caijing/apply_benefit/v1/` | POST |  | classes8.dex |
| `/reading/commerceapi/caijing/prize_info/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/chapter_end/resource/v1/` | POST |  | classes8.dex |
| `/reading/commerceapi/commerce/game_duration_task/v1/` | POST |  | classes8.dex |
| `/reading/commerceapi/commerce/game_task_page/v1/` | POST |  | classes8.dex |
| `/reading/commerceapi/commerce/gold_coin_deduction/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/commerce/playlet_same_product/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/commerce/report_game_duration/v1/` | POST |  | classes8.dex |
| `/reading/commerceapi/commerce/statistics/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/commerce/statistics_report/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/commerce/tab/v1/` | POST |  | classes6.dex, classes8.dex |
| `/reading/commerceapi/coupon/list/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/coupon/popup/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/coupon/prize/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/coupon/urge_bar/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/daily_lottery/entrance/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/entrance/info/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/game_center/info/v1/` | GET |  | classes3.dex, classes8.dex |
| `/reading/commerceapi/game_center/popup/info/v1/` | GET |  | classes3.dex, classes8.dex |
| `/reading/commerceapi/game_center/sign_in/info/v1/` | GET |  | classes3.dex, classes8.dex |
| `/reading/commerceapi/game_notice_bar/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/heating_plan/alive/v1/` | POST |  | classes6.dex, classes8.dex |
| `/reading/commerceapi/hot_rank/game/list/v1/` | POST |  | classes8.dex |
| `/reading/commerceapi/ip_draw/ack_pending_display/v1/` | POST |  | classes8.dex |
| `/reading/commerceapi/ip_draw/pending_display_result/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/ip_draw/retry_use/v1/` | POST |  | classes8.dex |
| `/reading/commerceapi/ip_draw/user_result_page/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/mall/promotion/page/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/mall/promotion/tab/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/micro_game/history/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/mini_program/history/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/mini_program/upload/v1/` | POST |  | classes8.dex |
| `/reading/commerceapi/mix_module/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/my_tab/info/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/order/status/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/payment_success/order_guide/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/playlet_same_product/cart/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/pop/up/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/product/card/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/product/refresh/v1/` | POST |  | classes8.dex |
| `/reading/commerceapi/reading_page_entrance/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/refresh/live/gold_text/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/report/impression/v1/` | POST |  | classes6.dex, classes8.dex |
| `/reading/commerceapi/report/show_click/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/send/msg/v1/` | POST |  | classes8.dex |
| `/reading/commerceapi/sidebar/game_entrance/v1/` | GET |  | classes6.dex, classes8.dex |
| `/reading/commerceapi/sstimor/cell/change/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/sstimor/tab/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/task/ack/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/task/info/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/token/info/v1/` | POST |  | classes8.dex |
| `/reading/commerceapi/union_game/awarding/v1/` | GET |  | classes8.dex |
| `/reading/commerceapi/user/quick_app/history/v1/` | POST |  | classes8.dex |
| `/reading/crypt/registerkey` | POST |  | classes14.dex |
| `/reading/distribution/album_pack_fields/:album_pack_channel/v1/` | POST |  | classes8.dex |
| `/reading/distribution/book_pack_fields/:book_pack_channel/v1/` | POST |  | classes6.dex, classes8.dex |
| `/reading/distribution/book_pack_fields/v1/` | POST |  | classes8.dex |
| `/reading/distribution/bookshelf_folder/detail/v1/` | POST |  | classes8.dex |
| `/reading/distribution/bookshelf_folder/item/update/v1/` | POST |  | classes6.dex, classes8.dex |
| `/reading/distribution/bookshelf_folder/list/v1/` | POST |  | classes8.dex |
| `/reading/distribution/bookshelf_folder/update/v1/` | POST |  | classes6.dex, classes8.dex |
| `/reading/distribution/category/landpage/v1/` | POST | ✅ | classes8.dex |
| `/reading/distribution/chapter_pack_fields/:chapter_pack_channel/v1/` | POST |  | classes8.dex |
| `/reading/distribution/content_extra_infos/:pack_channel/v1/` | POST |  | classes8.dex |
| `/reading/distribution/home/sidebar/v1/` | POST |  | classes6.dex, classes8.dex |
| `/reading/distribution/honor/detail/v1/` | POST |  | classes8.dex |
| `/reading/distribution/honor/entity/detail/v1/` | POST |  | classes8.dex |
| `/reading/distribution/recommend_stat/report/v1/` | POST |  | classes6.dex, classes8.dex |
| `/reading/distribution/room/pack/v1/` | POST |  | classes8.dex |
| `/reading/distribution/series_ai/ai_content/create/v1/` | POST |  | classes6.dex, classes8.dex |
| `/reading/distribution/series_ai/ai_content/delete/v1/` | POST |  | classes5.dex, classes8.dex |
| `/reading/distribution/series_ai/content/list/v1/` | POST |  | classes5.dex, classes8.dex |
| `/reading/distribution/series_ai/info/v1/` | POST |  | classes5.dex, classes8.dex |
| `/reading/distribution/series_ai/prompt_content/generate/v1/` | POST |  | classes8.dex |
| `/reading/distribution/sync/list/v1/` | GET |  | classes8.dex |
| `/reading/distribution/sync/status/v1/` | POST |  | classes8.dex |
| `/reading/distribution/sync/update/v1/` | POST |  | classes8.dex |
| `/reading/distribution/video_model_lite/v1/` | POST |  | classes8.dex |
| `/reading/distribution/video_pack_fields/:pack_channel/v1/` | POST |  | classes8.dex |
| `/reading/media/image/get/` | GET |  | classes8.dex |
| `/reading/media/image/resize/` | GET |  | classes8.dex |
| `/reading/media/image/transfer/` | POST |  | classes8.dex |
| `/reading/media/play/strategy/` | GET |  | classes8.dex |
| `/reading/media/streamtts/item/` | POST |  | classes8.dex |
| `/reading/media/streamtts/template/` | POST |  | classes8.dex |
| `/reading/media/video/info/` | POST |  | classes8.dex |
| `/reading/playlet/shortbuy_api/feedback/v1/` | POST |  | classes8.dex |
| `/reading/playlet/shortbuy_api/get_aigc_init_data/v1/` | POST |  | classes15.dex, classes8.dex |
| `/reading/playlet/shortbuy_api/get_aigc_seed_entrance/v1/` | POST |  | classes8.dex |
| `/reading/playlet/shortbuy_api/get_aigc_to_live_card/v1/` | POST |  | classes8.dex |
| `/reading/playlet/shortbuy_api/get_feed/v1/` | GET |  | classes8.dex |
| `/reading/playlet/shortbuy_api/mget_tag_info/v1/` | GET |  | classes8.dex |
| `/reading/playlet/shortbuy_api/report_impression/v1/` | POST |  | classes8.dex |
| `/reading/tradeapi/praise/callback/v1/` | POST |  | classes8.dex |
| `/reading/tradeapi/praise/callback_v2/v1/` | POST |  | classes8.dex |
| `/reading/tradeapi/vip/android_subscribe_pay_callback/v1/` | POST |  | classes8.dex |
| `/reading/tradeapi/vip/android_subscribe_pay_callback_v2/v1/` | POST |  | classes8.dex |
| `/reading/tradeapi/vip/callback/v1/` | POST |  | classes8.dex |
| `/reading/tradeapi/vip/callback_v2/v1/` | POST |  | classes8.dex |
| `/reading/tradeapi/vip/product/batch_preload/v1/` | GET |  | classes8.dex |
| `/reading/tradeapi/wallet/purchase/callback/v1/` | POST |  | classes8.dex |
| `/reading/tradeapi/wallet/recharge/callback/v1/` | POST |  | classes8.dex |
| `/reading/tradeapi/wallet/recharge/callback_v2/v1/` | POST |  | classes8.dex |
| `/reading/tradeapi/withdraw/callback/v1/` | POST |  | classes8.dex |

## 小说域其它（33）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/novel/reading/activity_api/user_behavioral_rewards/v1/` | — |  | classes10.dex |
| `/novel/trade/purchase/v1/set_auto_pay/` | — |  | classes21.dex |
| `/novel/im/action/report/v1/` | POST |  | classes21.dex |
| `/novel/im/conversation/apply/v1/` | POST |  | classes21.dex |
| `/novel/im/conversation/dismiss/v1/` | POST |  | classes21.dex |
| `/novel/im/conversation/member/mget/v1/` | POST |  | classes21.dex |
| `/novel/im/conversation/member/remove/v1/` | POST |  | classes21.dex |
| `/novel/im/conversation/member/search/v1/` | POST |  | classes21.dex |
| `/novel/im/conversation/mget/v1/` | POST |  | classes21.dex |
| `/novel/im/conversation/set/v1/` | POST |  | classes21.dex |
| `/novel/im/conversation/user/list/v1/` | POST |  | classes21.dex |
| `/novel/im/group/msg/list/v1/` | POST |  | classes21.dex |
| `/novel/im/mix/list/v1/` | POST |  | classes21.dex |
| `/novel/im/msg/ack/v1/` | POST |  | classes21.dex |
| `/novel/im/permission/get/v1/` | POST |  | classes21.dex |
| `/novel/reading/activity_api/get_multi_rank_list/v1/` | POST |  | classes8.dex |
| `/novel/reading/activity_api/get_rank_activity/v1/` | POST |  | classes8.dex |
| `/novel/reading/activity_api/get_rank_item_voting_record/v1/` | POST |  | classes8.dex |
| `/novel/reading/activity_api/get_script_content_user_data/v1/` | GET |  | classes8.dex |
| `/novel/reading/activity_api/get_script_list/v1/` | GET |  | classes8.dex |
| `/novel/reading/activity_api/get_script_voice_list/v1/` | GET |  | classes8.dex |
| `/novel/reading/activity_api/get_user_voting_record/v1/` | POST |  | classes8.dex |
| `/novel/reading/activity_api/search_rank_item/v1/` | POST |  | classes6.dex, classes8.dex |
| `/novel/reading/activity_api/user_behavioral_rewards/v1/` | POST |  | classes8.dex |
| `/novel/reading/activity_api/voting_rank_list/v1/` | POST |  | classes8.dex |
| `/novel/userapi/creator/activity/v1/` | GET |  | classes21.dex |
| `/novel/userapi/creator/book_list/v1/` | GET |  | classes21.dex |
| `/novel/userapi/creator/book_statistics/v1/` | GET |  | classes21.dex |
| `/novel/userapi/creator/user/v1/` | GET |  | classes21.dex |
| `/novel/userapi/medal/get/v1/` | GET |  | classes21.dex |
| `/novel/userapi/medal/set/v1/` | POST |  | classes21.dex |
| `/novel/userapi/user_title/wear/v1/` | POST |  | classes21.dex |
| `/novel/userapi/user_titles/v1/` | GET |  | classes21.dex |

## 离线包 novel-dragon（1）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/novel-dragon/feoffline/` | — |  | classes24.dex |

## 登录 passport（47）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/passport/` | — |  | classes11.dex, classes20.dex |
| `/passport/account/info/` | — |  | classes20.dex |
| `/passport/account/info/v2/` | — |  | classes11.dex, classes20.dex |
| `/passport/auth/bind/` | — |  | classes20.dex |
| `/passport/auth/bind_login/` | — |  | classes20.dex |
| `/passport/auth/bind_with_mobile/` | — |  | classes20.dex |
| `/passport/auth/bind_with_mobile_login/` | — |  | classes20.dex |
| `/passport/auth/change_bind/` | — |  | classes20.dex |
| `/passport/auth/get_oauth_token/` | — |  | classes20.dex |
| `/passport/auth/get_oauth_token/v2/` | — |  | classes20.dex |
| `/passport/auth/is_mobile_available/` | — |  | classes10.dex |
| `/passport/auth/login/` | — |  | classes20.dex |
| `/passport/auth/one_login/` | — |  | classes20.dex |
| `/passport/auth/one_login_continue/` | — |  | classes20.dex |
| `/passport/auth/one_login_only/` | — |  | classes20.dex |
| `/passport/auth/share_login/` | — |  | classes20.dex |
| `/passport/auth/switch_bind/` | — |  | classes20.dex |
| `/passport/auth/unbind/` | — |  | classes20.dex |
| `/passport/auth/update_authorize_info/` | — |  | classes20.dex |
| `/passport/can_aweme_quick_login/` | — |  | classes20.dex |
| `/passport/cancel/login/` | — |  | classes20.dex |
| `/passport/device/can_one_login/` | — |  | classes20.dex |
| `/passport/device/one_login/` | — |  | classes20.dex |
| `/passport/device/one_login_continue/` | — |  | classes20.dex |
| `/passport/device/trust_users/` | — |  | classes20.dex |
| `/passport/general/login_guiding_strategy/` | — |  | classes20.dex |
| `/passport/mobile/bind/v1/` | — |  | classes20.dex |
| `/passport/mobile/bind_login/` | — |  | classes20.dex |
| `/passport/mobile/change/v1/` | — |  | classes20.dex |
| `/passport/mobile/check_code/` | — |  | classes20.dex |
| `/passport/mobile/check_unusable/` | — |  | classes20.dex |
| `/passport/mobile/confirm_qrcode/` | — |  | classes20.dex |
| `/passport/mobile/send_code/v1/` | — | ✅ | classes20.dex |
| `/passport/mobile/sms_login/` | — | ✅ | classes20.dex |
| `/passport/mobile/sms_login_only/` | — | ✅ | classes20.dex |
| `/passport/mobile/validate_code/v1/` | — |  | classes20.dex |
| `/passport/open/auth/` | — |  | classes20.dex |
| `/passport/open/auth_info/v2/` | — |  | classes20.dex |
| `/passport/open/authorize/callback/` | — |  | classes14.dex |
| `/passport/open/host_mobile/one_auth/` | — |  | classes20.dex |
| `/passport/open/host_mobile/one_auth_only/` | — |  | classes20.dex |
| `/passport/open/host_mobile/one_auth_ticket/` | — |  | classes20.dex |
| `/passport/open/scan_qrcode/` | — |  | classes16.dex |
| `/passport/open/token_info/` | — |  | classes20.dex |
| `/passport/shark/safe_verify/` | — |  | classes20.dex |
| `/passport/token/beat/v2/` | — |  | classes11.dex, classes20.dex |
| `/passport/user/logout/` | — |  | classes10.dex, classes11.dex |

## 任务激励 luckycat（43）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/luckycat/:app` | — |  | classes5.dex |
| `/luckycat/activity` | — |  | classes22.dex |
| `/luckycat/activity/apply_token/` | — |  | classes22.dex |
| `/luckycat/activity/i/api/v:version/resource/gcall/launch/` | — |  | classes22.dex |
| `/luckycat/activity/phone_score/` | — |  | classes22.dex |
| `/luckycat/activity/refresh_act_id/` | — |  | classes22.dex |
| `/luckycat/activity/settings/get_dynamic_settings/` | — |  | classes22.dex |
| `/luckycat/activity/settings/get_polling_settings/` | — |  | classes22.dex |
| `/luckycat/activity/settings/get_static_settings/` | — |  | classes22.dex |
| `/luckycat/crossover/v:version/` | — |  | classes22.dex |
| `/luckycat/crossover/v:version/ack_install/` | — |  | classes22.dex |
| `/luckycat/novel` | — |  | classes22.dex, classes5.dex |
| `/luckycat/novel/1/desktop_widget/info` | — |  | classes24.dex |
| `/luckycat/novel/v1/appointment/page` | — |  | classes10.dex |
| `/luckycat/novel/v1/appointment/required` | — |  | classes10.dex |
| `/luckycat/novel/v1/popup/book_popup` | — |  | classes24.dex |
| `/luckycat/novel/v1/relation/big_invite_popup` | — |  | classes24.dex |
| `/luckycat/novel/v1/relation/invite_limit_popup` | — |  | classes24.dex |
| `/luckycat/novel/v1/relation/invite_popup` | — |  | classes24.dex |
| `/luckycat/novel/v1/task/done/daily_1min_earn_money` | — |  | classes10.dex |
| `/luckycat/novel/v1/task/done/daily_short_video_collect` | — |  | classes10.dex |
| `/luckycat/novel/v1/task/done/meal` | — |  | classes10.dex |
| `/luckycat/novel/v1/task/done/meal_continue_signin` | — |  | classes10.dex |
| `/luckycat/novel/v1/task/done/mix_task_collect` | — |  | classes10.dex |
| `/luckycat/novel/v1/task/done/open_push_permission` | — |  | classes10.dex |
| `/luckycat/novel/v1/task/done/popup_redelivery` | — |  | classes10.dex |
| `/luckycat/novel/v1/task/done/push_recall` | — |  | classes7.dex |
| `/luckycat/novel/v1/task/done/treasure_task` | — |  | classes10.dex |
| `/luckycat/novel/v1/task/meal_v2/page` | — |  | classes10.dex |
| `/luckycat/novel/v1/task/push_recall/detail` | — |  | classes7.dex |
| `/luckycat/novel/v1/task/split_coin_activity_detail` | — |  | classes10.dex |
| `/luckycat/novel/v1/task/task_status` | — |  | classes24.dex, classes7.dex |
| `/luckycat/novel/v1/task/welfare_second_floor_page` | — |  | classes7.dex |
| `/luckycat/novel/v1/user/old_user_lost` | — |  | classes24.dex |
| `/luckycat/novel/v1/user/tabs` | — |  | classes24.dex |
| `/luckycat/novel/v1/wallet/cash_balance` | — |  | classes24.dex, classes7.dex |
| `/luckycat/:app/v1/resource/pendant_plan` | POST |  | classes22.dex |
| `/luckycat/novel/v1/desktop_widget/benefits` | GET |  | classes8.dex |
| `/luckycat/novel/v1/material/data` | POST |  | classes8.dex |
| `/luckycat/novel/v1/popup/referral_user_popup` | GET |  | classes8.dex |
| `/luckycat/novel/v1/popup/take_cash_low_active_popup` | GET |  | classes8.dex |
| `/luckycat/novel/v:version/task/done/book_olympics` | POST |  | classes8.dex |
| `/luckycat/novel/v:version/task/done/redpack` | POST |  | classes8.dex |

## 平台服务 service（设备注册/设置/日志/广告）（10）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/service/1/app_notice_status/` | — |  | classes21.dex |
| `/service/1/client_context/` | — |  | classes19.dex |
| `/service/1/update_token/` | — |  | classes21.dex |
| `/service/14/app_ad/` | — |  | classes11.dex |
| `/service/2/app_alert_check/` | — |  | classes19.dex |
| `/service/2/app_log/` | — |  | classes11.dex, classes12.dex, classes19.dex |
| `/service/2/app_log_test/` | — |  | classes11.dex, classes12.dex, classes19.dex |
| `/service/2/device_register/` | — | ✅ | classes11.dex, classes19.dex |
| `/service/2/device_sdk/kite/` | — |  | classes20.dex |
| `/service/settings/v3/` | — |  | classes21.dex, classes22.dex, classes5.dex |

## 广告 api（61）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/api/ad/feedback/privacy/` | — |  | classes24.dex |
| `/api/ad/personality_switch/` | — |  | classes24.dex |
| `/api/ad/preload_ad/v2/` | — |  | classes11.dex |
| `/api/ad/preload_ad/v3` | — |  | classes11.dex |
| `/api/ad/refresh/v1/` | — |  | classes11.dex |
| `/api/ad/share/v1/` | — |  | classes11.dex |
| `/api/ad/splash/` | — |  | classes11.dex |
| `/api/ad/splash/novelapp/v14` | — |  | classes5.dex |
| `/api/ad/splash/stat/ack/v1/` | — |  | classes11.dex |
| `/api/ad/splash/stock/realtime/v14/` | — |  | classes11.dex |
| `/api/ad/splash_shake_switch/` | — |  | classes24.dex |
| `/api/ad/union/sdk/upload/app_info/` | — |  | classes24.dex |
| `/api/ad/v1/aggregate/` | — |  | classes5.dex |
| `/api/ad/v1/banner/` | — |  | classes5.dex |
| `/api/ad/v1/byte_union/recall_ads/` | — |  | classes13.dex |
| `/api/ad/v1/dislike/` | — |  | classes13.dex, classes18.dex, classes24.dex, classes5.dex |
| `/api/ad/v1/inspire/` | — |  | classes5.dex |
| `/api/ad/v1/inspire_send/` | — |  | classes13.dex |
| `/api/ad/v1/recall_ads/banner/` | — |  | classes11.dex, classes5.dex |
| `/api/ad/v1/report/` | — |  | classes13.dex, classes24.dex |
| `/api/ad/v1/report/feedback/` | — |  | classes13.dex, classes24.dex, classes5.dex |
| `/api/ad/v1/splash/stock/` | — |  | classes11.dex |
| `/api/ad/v2/ack/splash/` | — |  | classes11.dex |
| `/api/analysis/v1/get_screenshot_authorize_pop_quota` | — |  | classes19.dex |
| `/api/analysis/v1/upload_screenshot_indicator` | — |  | classes19.dex |
| `/api/apps/BatchMeta` | — |  | classes19.dex |
| `/api/apps/desktop_app/get_desktop_app` | — |  | classes19.dex |
| `/api/apps/history` | — |  | classes19.dex |
| `/api/apps/live_meta` | — |  | classes19.dex |
| `/api/apps/user/openid` | — |  | classes19.dex |
| `/api/apps/v3/meta` | — |  | classes19.dex |
| `/api/checkLogSensitiveWords/` | — |  | classes19.dex |
| `/api/neptune/v3/sdk/PackLiveSDK` | — |  | classes13.dex |
| `/api/novel/account/v1/info/` | — |  | classes21.dex |
| `/api/novel/account/v1/readtime/report/` | — |  | classes21.dex |
| `/api/novel/account/v1/vip/info/` | — |  | classes21.dex |
| `/api/novel/book/bookshelf/check/v1/` | — |  | classes21.dex |
| `/api/novel/channel/common/get_ab_value/v1/` | — |  | classes21.dex |
| `/api/timon_config_proxy/config/get` | — |  | classes22.dex |
| `/api/v2/commerce/bff/homepage` | — |  | classes15.dex |
| `/api/v2/commerce/bff/homepage/favorite/feed` | — |  | classes15.dex |
| `/api/verify/v1` | — |  | classes22.dex |
| `/api/novel/origin/album/add/v1/` | POST |  | classes8.dex |
| `/api/novel/origin/album/add_video_order/v1/` | POST |  | classes8.dex |
| `/api/novel/origin/album/delete/v1/` | POST |  | classes8.dex |
| `/api/novel/origin/album/get_video_list/v1/` | GET |  | classes8.dex |
| `/api/novel/origin/album/list/v1/` | GET |  | classes8.dex |
| `/api/novel/origin/album/modify/v1/` | POST |  | classes8.dex |
| `/api/novel/origin/album/modify_order/v1/` | POST |  | classes8.dex |
| `/api/novel/origin/album/search_video_list/v1/` | GET |  | classes8.dex |
| `/api/novel/origin/album/search_video_list_v2/v1/` | GET |  | classes8.dex |
| `/api/novel/origin/text/check/v1/` | GET |  | classes8.dex |
| `/api/novel/origin/video/account/relation/v1/` | GET |  | classes8.dex |
| `/api/novel/origin/video/add/v1/` | POST |  | classes8.dex |
| `/api/novel/origin/video/delete/v1/` | GET |  | classes8.dex |
| `/api/novel/origin/video/list/v1/` | GET |  | classes8.dex |
| `/api/novel/origin/video/modify/v1/` | POST |  | classes8.dex |
| `/api/novel/origin/video/sync/v1/` | POST |  | classes8.dex |
| `/api/origin/activity/novel/apply_attend_activity/v0/` | POST |  | classes21.dex |
| `/api/origin/activity/novel/book_can_attend_list/v0/` | GET |  | classes21.dex |
| `/api/origin/activity/novel/libra_propeties/v0` | GET |  | classes21.dex |

## 抖音互通 aweme（16）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/aweme/ug/` | — |  | classes22.dex |
| `/aweme/ug/flower/` | — |  | classes22.dex |
| `/aweme/ug/task/` | — |  | classes22.dex |
| `/aweme/ug/task/do_action` | — |  | classes22.dex |
| `/aweme/ughun/` | — |  | classes22.dex |
| `/aweme/v1/general/search` | — |  | classes18.dex |
| `/aweme/v1/store` | — |  | classes18.dex |
| `/aweme/v2/commerce` | — |  | classes18.dex |
| `/aweme/v2/commerce/activity/list/impression` | — |  | classes18.dex |
| `/aweme/v2/commerce/bff/homepage` | — |  | classes18.dex |
| `/aweme/v2/commerce/bff/homepage/` | — |  | classes18.dex |
| `/aweme/v2/commerce/bff/homepage/favorite/feed` | — |  | classes18.dex |
| `/aweme/v2/search` | — |  | classes18.dex |
| `/aweme/v2/shop` | — |  | classes18.dex |
| `/aweme/v2/sku` | — |  | classes18.dex |
| `/aweme/v3/shop` | — |  | classes18.dex |

## 直播 webcast（7）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/webcast/content_open/feed/` | — |  | classes18.dex |
| `/webcast/mono/h5/open_basic_h5_saas/template/pages/re_clock_in_popup.html` | — |  | classes20.dex |
| `/webcast/mono/lynx/revenue_gift_panel_saas/pages/task_touch/template.js` | — |  | classes20.dex |
| `/webcast/openapi/distribution/check_user_live_status/` | — |  | classes24.dex |
| `/webcast/openapi/feed` | — |  | classes5.dex |
| `/webcast/openapi/feed/` | — |  | classes18.dex |
| `/webcast/openapi/room/info_by_scene/` | — |  | classes18.dex |

## 监控 monitor（25）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/monitor` | — |  | classes11.dex |
| `/monitor/appmonitor/v2/settings` | — |  | classes11.dex, classes13.dex, classes22.dex |
| `/monitor/appmonitor/v3/settings` | — |  | classes19.dex |
| `/monitor/appmonitor/v4/settings` | — |  | classes19.dex |
| `/monitor/appmonitor/v5/batch_settings` | — |  | classes12.dex |
| `/monitor/collect/` | — |  | classes11.dex, classes13.dex, classes19.dex, classes22.dex |
| `/monitor/collect/batch` | — |  | classes24.dex |
| `/monitor/collect/batch/` | — |  | classes12.dex, classes19.dex |
| `/monitor/collect/c/biz_exception` | — |  | classes19.dex |
| `/monitor/collect/c/cloudcontrol/get` | — |  | classes11.dex, classes24.dex |
| `/monitor/collect/c/code_coverage` | — |  | classes12.dex, classes19.dex |
| `/monitor/collect/c/core_dump_collect` | — |  | classes19.dex |
| `/monitor/collect/c/crash` | — |  | classes19.dex |
| `/monitor/collect/c/exception` | — |  | classes12.dex, classes19.dex, classes21.dex |
| `/monitor/collect/c/exception/dump_collection` | — |  | classes19.dex |
| `/monitor/collect/c/logcollect` | — |  | classes12.dex, classes19.dex |
| `/monitor/collect/c/mom_dump_collect` | — |  | classes21.dex |
| `/monitor/collect/c/mom_dump_collect_part/confirm` | — |  | classes21.dex |
| `/monitor/collect/c/mom_dump_collect_part/init` | — |  | classes21.dex |
| `/monitor/collect/c/mom_dump_collect_part/upload` | — |  | classes21.dex |
| `/monitor/collect/c/native_bin_crash` | — |  | classes19.dex |
| `/monitor/collect/c/trace_collect` | — |  | classes12.dex |
| `/monitor/collect/quota_status` | — |  | classes19.dex |
| `/monitor/collect/zstd_dict/` | — |  | classes12.dex |
| `/monitor/data/validation` | — |  | classes18.dex |

## 隐私合规 privacy（7）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/privacy/info/deletion/activate_account` | — |  | classes24.dex |
| `/privacy/setting/change` | — |  | classes24.dex |
| `/privacy/setting/query` | — |  | classes24.dex |
| `/privacy/teen_mode/close` | — |  | classes24.dex |
| `/privacy/teen_mode/open` | — |  | classes24.dex |
| `/privacy/teen_mode/query` | — |  | classes24.dex |
| `/privacy/teen_mode/verify` | — |  | classes24.dex |

## 用户中心 usercenter（15）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/usercenter` | — |  | classes12.dex |
| `/usercenter/balance/detail` | — |  | classes12.dex |
| `/usercenter/balance/list` | — |  | classes12.dex |
| `/usercenter/bindphone/confirmIdentity` | — |  | classes12.dex |
| `/usercenter/bindphone/relationInfo` | — |  | classes12.dex |
| `/usercenter/cards` | — |  | classes12.dex |
| `/usercenter/cards/detail` | — |  | classes12.dex |
| `/usercenter/help/walletFaq` | — |  | classes12.dex |
| `/usercenter/home` | — |  | classes12.dex |
| `/usercenter/member` | — |  | classes12.dex |
| `/usercenter/member/info` | — |  | classes12.dex |
| `/usercenter/paymng` | — |  | classes12.dex |
| `/usercenter/paymng/mobilepass` | — |  | classes12.dex |
| `/usercenter/paymng/protocol` | — |  | classes12.dex |
| `/usercenter/setpass` | — |  | classes12.dex |

## 云推送 cloudpush（14）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/cloudpush/batch_update/` | — |  | classes21.dex |
| `/cloudpush/callback/android_red_badge/` | — |  | classes11.dex |
| `/cloudpush/callback/client_click/` | — |  | classes21.dex |
| `/cloudpush/callback/client_show/` | — |  | classes21.dex |
| `/cloudpush/clear_notification/` | — |  | classes21.dex |
| `/cloudpush/event_sync/` | — |  | classes21.dex |
| `/cloudpush/fetch_business_status/` | — |  | classes21.dex |
| `/cloudpush/ping/` | — |  | classes21.dex |
| `/cloudpush/promotion/keep_alive/` | — |  | classes12.dex |
| `/cloudpush/pull_alter/sdk_strategy` | — |  | classes18.dex |
| `/cloudpush/pull_compose_data/` | — |  | classes12.dex |
| `/cloudpush/update_frontier_setting/` | — |  | classes21.dex |
| `/cloudpush/update_sender/` | — |  | classes21.dex |
| `/cloudpush/user_push_replace/` | — |  | classes21.dex |

## 活动 activity（H5 路由）（11）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/activity/protocol/account` | — |  | classes12.dex |
| `/activity/protocol/auth` | — |  | classes12.dex |
| `/activity/protocol/cancelAccount` | — |  | classes12.dex |
| `/activity/protocol/CMB` | — |  | classes12.dex |
| `/activity/protocol/hzAccount` | — |  | classes12.dex |
| `/activity/protocol/privacy` | — |  | classes12.dex |
| `/activity/protocol/psbc` | — |  | classes12.dex |
| `/activity/protocol/quickpay` | — |  | classes12.dex |
| `/activity/protocol/sms` | — |  | classes12.dex |
| `/activity/protocol/usercenter/help/protocol` | — |  | classes12.dex |
| `/activity/protocol/year` | — |  | classes12.dex |

## 支付网关 gateway/bytepay/cashier2/cashdesk（45）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/gateway-bytepay/bytepay/agreement_auth/load_home_sign_page` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/agreement_auth/load_login_sign_page` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/agreement_auth/sign_confirm` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/cashdesk/` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/cashdesk/get_verify_info` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/cashdesk/growth_decision` | — |  | classes12.dex |
| `/gateway-bytepay/bytepay/cashdesk/out_pre_trade_info_query_pb` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/cashdesk/outer_pre_trade_info_query` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/cashdesk/pay_new_card` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/cashdesk/pre_trade` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/cashdesk/query_pay_order_info` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/cashdesk/query_pay_type` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/cashdesk/query_withdraw_fee` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/cashdesk/trade_confirm` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/cashdesk/trade_create_again` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/cashdesk/trade_query` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/cashdesk/user_verify` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/cashdesk/verify_and_one_key_pay` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/member_apply/` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/member_product/` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/member_product/bind_byte_pay_account` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/member_product/get_card_no_by_ocr` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/member_product/get_live_detection_ticket_v2` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/member_product/ocr_id_photo` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/member_product/query_sign_template` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/member_product/set_member_first_pay_type` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/member_product/upload_face_video` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/member_product/verify_live_detection_result` | — |  | classes19.dex |
| `/gateway-bytepay/bytepay/pre_check/` | — |  | classes19.dex |
| `/gateway-bytepay/totp_proofread` | — |  | classes19.dex |
| `/gateway-cashier2/tp/cashier/` | — |  | classes19.dex |
| `/gateway-cashier2/tp/cashier/direct_confirm` | — |  | classes12.dex |
| `/gateway-cashier2/tp/cashier/event_upload` | — |  | classes12.dex |
| `/gateway-cashier2/tp/cashier/pre_sign_and_consult` | — |  | classes12.dex |
| `/gateway-cashier2/tp/cashier/query_sign_info` | — |  | classes12.dex, classes19.dex |
| `/gateway-cashier2/tp/cashier/sign_confirm` | — |  | classes12.dex |
| `/gateway-cashier2/tp/cashier/sign_create` | — |  | classes12.dex |
| `/gateway-cashier2/tp/cashier/sign_query` | — |  | classes12.dex |
| `/gateway-cashier2/tp/cashier/trade_confirm` | — |  | classes12.dex, classes19.dex |
| `/gateway-cashier2/tp/cashier/trade_create` | — |  | classes12.dex, classes19.dex |
| `/gateway-cashier2/tp/cashier/trade_create_by_token` | — |  | classes12.dex, classes19.dex |
| `/gateway-cashier2/tp/cashier/trade_create_pb` | — |  | classes12.dex, classes19.dex |
| `/gateway-cashier2/tp/cashier/trade_query` | — |  | classes12.dex, classes19.dex |
| `/gateway/tp/quick_pay/deduct_again` | — |  | classes12.dex |
| `/gateway/tp/quick_pay/trade_query` | — |  | classes12.dex, classes19.dex |

## 其它（gkx 配置/video/praise/bookapi 裸路径等）（37）

| 路径 | 方法 | 在用 | 来源 dex |
| --- | --- | :-: | --- |
| `/bookapi/bookmall/cell/change` | — |  | classes24.dex |
| `/bookapi/bookmall/tab` | — |  | classes24.dex |
| `/bookapi/resource/config/` | — |  | classes24.dex |
| `/bookapi/search/tab` | — |  | classes5.dex |
| `/cashdesk_withdraw` | — |  | classes12.dex |
| `/cashdesk_withdraw/alipaybind` | — |  | classes12.dex |
| `/cashdesk_withdraw/balance` | — |  | classes12.dex |
| `/cashdesk_withdraw/error` | — |  | classes12.dex |
| `/cashdesk_withdraw/faq` | — |  | classes12.dex |
| `/cashdesk_withdraw/orderStatus` | — |  | classes12.dex |
| `/cashdesk_withdraw/recordList` | — |  | classes12.dex |
| `/commerceapi/commerce/tab` | — |  | classes2.dex, classes24.dex, classes5.dex |
| `/get_domains/v5` | — |  | classes19.dex |
| `/get_domains/v5/` | — |  | classes19.dex, classes22.dex |
| `/gkx/api/resource/v7/coldstart` | — |  | classes20.dex, classes5.dex |
| `/gkx/api/resource/v7/custom` | — |  | classes20.dex, classes5.dex |
| `/gkx/api/resource/v7/polling` | — |  | classes20.dex, classes5.dex |
| `/gkx/api/settings/v3` | — |  | classes20.dex, classes5.dex |
| `/praise/create_order/` | — |  | classes15.dex, classes9.dex |
| `/praise/status/` | — |  | classes9.dex |
| `/video/live/qos/v2/ipSettings` | — |  | classes13.dex |
| `/video/openapi/v1/` | — |  | classes13.dex |
| `/video/play/` | — |  | classes13.dex |
| `/video/social/add_friend/red_code/scan/v1/` | POST |  | classes21.dex |
| `/video/social/add_friend_page/v1/` | GET |  | classes21.dex |
| `/video/social/block_user/v1/` | POST |  | classes21.dex |
| `/video/social/mget_contact/v1/` | GET |  | classes21.dex |
| `/video/social/mupload_contact/v1/` | POST |  | classes21.dex |
| `/video/works/album/create/v1/` | POST |  | classes21.dex |
| `/video/works/album/delete/v1/` | POST |  | classes21.dex |
| `/video/works/album/detail/v1/` | POST |  | classes5.dex |
| `/video/works/album/modify/v1/` | POST |  | classes21.dex |
| `/video/works/album/search_video_list/v1/` | GET |  | classes5.dex |
| `/video/works/album/video_composition/modify/v1/` | POST |  | classes21.dex |
| `/video/works/text/check/v1/` | GET |  | classes5.dex |
| `/video/works/user/album/list/v1/` | GET |  | classes21.dex |
| `/video/works/user/album_to_drama/v1/` | GET |  | classes21.dex |

