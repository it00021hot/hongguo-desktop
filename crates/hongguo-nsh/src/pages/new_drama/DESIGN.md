# 新剧页布局对照

生产与离线 fixture 共用推荐 presentation。基线为原 Tauri NewDramaPage / NewDramaRecommends / NewDramaCard，默认1280×840：顶栏推荐/日历；内容左右16、上12；频道24高；间距12；五列卡片列/行距16。卡片封面3:4，标题14/20、分类12/16、评分12/16，正文内边距8。

## 状态与 motion inventory（实现前）

| 状态变化                    | 工具 / 令牌                                                                                      | reduced motion | 验证                                                      |
| --------------------------- | ------------------------------------------------------------------------------------------------ | -------------- | --------------------------------------------------------- |
| 顶栏与频道 hover/press/选中 | ui::header_tab / ui::history_filter 的FAST transition、语义primary/accent/ring                   | duration0      | 原生两主题已验证点击/选中，hover/reduced motion未单独验收 |
| 卡片hover/focus/press       | border foreground30%、ring outline、FAST transition；不缩放封面                                  | duration0      | 原生两主题已验证首卡进入详情；hover/focus未单独验收       |
| 推荐/日历切换               | 原容器即时切换、持有独立资源；顶栏状态随mode更新                                                 | 无额外位移     | 新非空日历待原生两主题对照                                |
| 频道切换/下一页             | 保留原资源loading/error/retry，虚拟网格按series id换内容；数据直接更新不做入场动画以保留滚动稳定 | 同上           | fixture男频/全部切换已通过，真实分页未运行                |
| 下载计数                    | completed任务投影，徽标直接更新不做数值动画                                                      | 同上           | 首卡完成1集徽标两主题目检；真实任务变化未运行             |
| 弹层/拖拽/持续播放          | 无                                                                                               | 无             | 不适用                                                    |

## 范围与未对齐

生产请求/身份变化/分页保留；预览只读共同JSON/帧，不启动数据库、网络或播放器。日历与推荐均使用生产展示组件，日历固定数据包含已上线、待上线未预约和已预约三行，不以空态替代列表验收。窄屏仍用独立移动导航，日期按钮高度56，动作在移动端保留48触达。

推荐频道在右侧，切换频道重建虚拟网格归零；推荐和日历均通过虚拟列表接近尾部事件自动分页。日历切日期保留旧资料并使用共享RefreshShade降低至50%透明度、阻断旧条目操作，200ms过渡（reduced motion为0）。同频道跨主导航滚动恢复与真实分页仍需单独验证。

## 日历与共享行卡

基线是生产NewCalendarView、CalendarRow、SeriesRowCard。默认1280×840，内容起点x240/y56：日期56高、日期字号14/星期11，8个日期默认实宽56、间距8；日期条下方间距16。列表卡宽1024、外边距12、半径14，海报80×106.67、半径10；每行间距8。标题含上线徽章，元信息按热度、分类、星评分、集数排列；右侧是本地时区上线时间及播放/详情或预约。

排行榜与日历共用`ui::series_row`骨架，颜色、圆角、hover/focus和阴影由同一层持有；日历专用日期、状态徽章及动作通过`ui::calendar`组装。日期解析与时间格式复用chrono，星期使用i18n，非北京时区也按本机时区显示，未定档不伪造时间。

| 日历状态变化         | 工具与反馈                                                                  | reduced motion | 验证                                           |
| -------------------- | --------------------------------------------------------------------------- | -------------- | ---------------------------------------------- |
| 日期选中/hover/focus | RadioGroup语义与primary选中、ring焦点，FAST颜色过渡                         | duration0      | 明暗原生切日/空态/返回通过；完整键盘与读屏未验 |
| 切日刷新             | `ui::refresh_shade`，旧内容50%透明且禁点，200ms恢复                         | duration0      | 原生切日行为通过；真实API异步状态需另验        |
| 行卡hover/focus      | 共享骨架foreground30%边框、hover阴影，FAST                                  | duration0      | 明暗截图已目检；完整键盘未验                   |
| 预约提交             | 单项pending禁用，成功才改已预约并刷新成员；失败显示真实错误，身份变化清状态 | 即时状态更新   | fixture预约→已预约原生通过；真实账号未运行     |
| 列表翻页/切日        | 有界虚拟行、series id键，切日重建滚动区，无到达动画                         | 相同           | 元数据分页去重单测已通过；真实多页未运行       |

共同非媒体数据：`preview/fixtures/calendar.json`；Tauri参考也读此文件。固定默认日2026-11-11，两个预览使用相同基准日；封面只引用忽略的本地帧。

## Finishing checklist

- [x] 深浅主题截图：radius、typography、spacing、alignment、color。卡片首排y92..423、次排y439、列距16；主题语义色一致。字体保留设计权重，原生与Web平滑观感有差异。
- [ ] hover、pressed、focus、loading/error/empty状态及motion。
- [ ] 键盘导航/Enter/Space和辅助功能名称。
- [ ] 虚拟网格回收后内容与series id保持一致。

证据：`preview/layout/NewDrama.test.tsx` 原生macOS两项通过，`target/nsh-preview/new-{dark,light}-expanded-1280x840.png` 已目检。原生PNG内嵌显示器ICC（本机X34E），与Chrome sRGB截图比较颜色前须尊重ICC；本机raw背景23/卡片30转sRGB后分别为24/33，恰好与Tauri令牌一致，不能据raw通道值补色。

非空日历`Calendar.test.tsx`明暗2/2通过，连同推荐本批4/4。最后footer间距按真实尾部哨兵补8，`AlignmentFinishing.test.tsx`明暗2/2已补图并目检：日期x240/y56，高56、宽56；三行y128/269/410，图片80×106.67，右侧时间/按钮/详情齐线，“没有更多了”回到参照位置。radius、typography、spacing、alignment、color五组已检查，三种上线/预约状态都保留。loading/error的真实API、长列表回收、减少动态效果、Windows/Android/iOS及系统读屏仍未验，不将离线桌面图代替这些结果。
