/**
 * 与 Rust serde 模型对应的 zod schema（排行 / 新剧 / 上新日历 / 预约域）。
 */
import { z } from 'zod';

/** 榜单条目（Rust `rank::RankItem`）。 */
const rankItemSchema = z.object({
  seriesId: z.string(),
  title: z.string(),
  cover: z.string(),
  vid: z.string(),
  /** 榜单名次（从 1 起；0 表示接口没给，如预约榜） */
  rank: z.number().int().nonnegative(),
  /** "玄幻·全200集" 形态的副标题 */
  subTitle: z.string(),
  score: z.number(),
  playCnt: z.number(),
  episodeCnt: z.number().int().nonnegative(),
  /** 榜单热点文案（"13707万最高热度"） */
  recText: z.string(),
  /** 次级信息（"258.7万收藏"） */
  secondaryInfos: z.array(z.string()),
  description: z.string(),
  tags: z.array(z.string()),
  /** 未上线（分集数 0）：行内显示预约按钮、点击进详情而非播放 */
  upcoming: z.boolean(),
  /** 当前账号已预约（榜单条目 online_subscribed 下发） */
  reserved: z.boolean(),
  /** 季徽（「第N季」，sub_title_list 提取；无则空串） */
  season: z.string(),
});
export type RankItem = z.infer<typeof rankItemSchema>;

/** 筛选面板选项（id 为空 = 「总榜」，即清除筛选）。 */
const rankPanelItemSchema = z.object({
  id: z.string(),
  name: z.string(),
});

/** 面板一行（综合 / 时代背景 / 主题情节 / 角色设定…）。 */
const rankPanelRowSchema = z.object({
  name: z.string(),
  items: z.array(rankPanelItemSchema),
});

/** 内容 tab 下的一个子榜（自带筛选面板 schema）。 */
const rankSubListSchema = z.object({
  id: z.string(),
  name: z.string(),
  /** 子榜描述行（如 "10月4日已更新·基于红果观看/互动以及个人兴趣排序"） */
  description: z.string(),
  panel: z.array(rankPanelRowSchema),
});
export type RankSubList = z.infer<typeof rankSubListSchema>;

/** 顶部内容 tab（全部/真人剧/漫剧/AI剧/系列剧；演员榜无剧集数据不提供）。 */
const rankTabSchema = z.object({
  id: z.string(),
  name: z.string(),
  subs: z.array(rankSubListSchema),
});
export type RankTab = z.infer<typeof rankTabSchema>;

export const rankPageSchema = z.object({
  items: z.array(rankItemSchema),
  /** 内容 tab → 子榜 → 筛选面板的选项表（响应 cell_selector 展开） */
  tabs: z.array(rankTabSchema),
  /** 分页游标（每页固定 20 条、相邻页重叠 10 条需去重；has_more=false 到底） */
  hasMore: z.boolean().default(false),
  nextOffset: z.number().default(0),
  /** 浏览会话标识：翻页原样回传（服务端按它维持榜单上下文） */
  sessionId: z.string().default(''),
});
export type RankPage = z.infer<typeof rankPageSchema>;

/** 上新日历条目（Rust `rank::CalendarItem`）。 */
const calendarItemSchema = z.object({
  seriesId: z.string(),
  title: z.string(),
  cover: z.string(),
  vid: z.string(),
  score: z.number(),
  playCnt: z.number(),
  episodeCnt: z.number().int().nonnegative(),
  description: z.string(),
  category: z.string(),
  recTags: z.array(z.string()),
  /** 排期上线时间（unix 秒；0 = 未定档） */
  publishTime: z.number(),
  isOnline: z.boolean(),
  /** 当前账号是否已预约（预约列表形态下发；日历形态恒 false） */
  hasSubscribed: z.boolean(),
});
export type CalendarItem = z.infer<typeof calendarItemSchema>;

export const calendarPageSchema = z.object({
  items: z.array(calendarItemSchema),
  /** "20261003" 形式的可选日期 */
  dates: z.array(z.string()),
  defaultDate: z.string(),
  hasMore: z.boolean(),
  nextOffset: z.number(),
  /** 预约列表两个 tab 的计数（日历形态恒 0） */
  onlineTotal: z.number(),
  offlineTotal: z.number(),
});
export type CalendarPage = z.infer<typeof calendarPageSchema>;
