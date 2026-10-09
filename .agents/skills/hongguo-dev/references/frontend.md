# 前端开发规范

## 目录归属判定（先探测结构状态）

仓库正从旧结构迁往 `docs/refactor-plan.md` 的目标结构。动手前看目录：已是 `src/pages/ + src/service/` 按新规则；还是 `src/routes/ + src/lib/` 就按现状就近，别往旧巨石文件（`lib/queries.ts`）里堆新代码。新规则按顺序自问，命中即止：

1. 只服务一个路由页面 → `pages/<域>/` 就近（复杂页面才有 components/hooks 子目录）
2. 复杂、跨页面复用的能力 → `features/`（player/update 这类体量才配进）
3. 和 Rust 交互/服务端数据 → `service/`（旧结构 `lib/ipc` + `lib/queries.ts`）
4. 跨页 hook → `hooks/`；全局客户端状态 → `stores/`；无业务纯函数 → `utils/`
5. shadcn 件 `components/ui/`（别手改生成风格）；布局壳 `components/layout/`；跨页展示件 `components/common/`
6. `cn()` 只在 `lib/utils.ts`，别另起一份

反面清单：不建巨石文件；不造只有一行转发的包装层；同一业务规则不许两份实现。

## 文件与命名

- 文件 kebab-case（`player-controls.tsx`）；组件 PascalCase 导出；目录名用域名词（`download/`、`new-drama/`）。
- 路由文件是薄壳（`createFileRoute` + 渲染页面组件），真实页面在 features/pages 实现目录。
- `routeTree.gen.ts` 是生成物不要手改；路由目录变了要同步 vite.config.ts 的 `tanstackRouter` 配置。

## 组件写法

- 函数组件 + hooks，无 class。一个文件多个小组件是历史欠账，新文件一组件一文件。
- 注释风格：解释「为什么」而不是「做什么」——本仓库的注释是决策记录（如 ui store 里「miniScreen/pinned 不持久化：窗口几何的存/恢复在后端」），照这个密度写。
- 错误展示用 sonner toast；文案一律走 i18n，不硬编码中文字符串。
- 乐观更新型交互（点赞/收藏）用 react-query 的 `onMutate` 模式（范例在 `queries.ts` 的 interact 域）。

## TanStack Query（服务端状态唯一去处）

**key 纪律**：所有 key 从 `keys` 工厂出，不在组件里手写字符串——否则 invalidate 时漏 key 界面不刷新。新查询先加工厂：

```ts
export const keys = {
  seriesProgress: (id: string) => ['series-progress', id] as const,
  // ...
} satisfies Record<string, unknown>;
```

**查询/变更模式**（照现状抄）：

```ts
export function useSaveSettings() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: settings.save,
    onSuccess: (saved) => {
      qc.setQueryData(keys.settings, saved); // 已知结果直接写
      void qc.invalidateQueries({ queryKey: keys.queueStatus }); // 派生数据失效
    },
  });
}
```

- **事件驱动失效**：后端事件到了就 invalidate 对应 key（范例 `useSeriesEpisodes` 里 `useEvent(seriesArchiveUpdated)` 回调中 `invalidateQueries`）。
- **staleTime 有意识地决定**：`useSeriesProgress` 故意不给 staleTime（详情页每次挂载必须现读本地真值），别无脑抄别人的配置。
- 无限滚动统一走 `useInfiniteStream` 封装（items/hasMore/loadMore 形状，翻页失败不炸整页）。
- 长任务挂起兜底：invoke 加 `Promise.race` 30s 超时（Rust 热重载会丢在途 invoke 的应答，promise 永不 settle）。
- 服务端数据不进 zustand、不进组件 state——那是两套缓存打架的根源。

## Zustand（跨组件 UI/会话状态）

一个关注点一个 store（`stores/player|ui|theme|locale`）。写法照 `ui.ts`：

```ts
export const useUiStore = create<UiState>()(
  persist(
    (set, get) => ({ sidebarCollapsed: true, toggleSidebar: () => set({ sidebarCollapsed: !get().sidebarCollapsed }), ... }),
    { name: 'hongguo-ui', partialize: (s) => ({ /* 只持久化该持久的 */ }) },
  ),
);
```

- store 只放「纯 UI 偏好/会话态」，不含业务数据；要不要持久化是显式决策，写进 `partialize` 和注释。
- 窗口级状态（置顶/小屏几何）的真值在后端，前端 store 只是镜像，别在前端单方面改。

## i18n

- `t('path.to.key')` 取词条、`tf(path, { vars })` 插值；**资源结构由 zh-CN.json 决定，en-US.json 必须与之完全一致**（type Dict = typeof zhCN 编译期约束）——加词条两个文件都要加。
- 后端错误 kind 就是 i18n key（`error.network`…），新 AppError 变体要补 `error.*` 词条（`error.auth` 例外，故意无译文）。
- 缺词条回落显示 key 本身——比 undefined 好定位。

## 路由与搜索参数（有真坑）

- TanStack Router 文件式路由；`main.tsx` 自定义 `parseSearch/stringifySearch` 防**剧集 id（i64 精度）在 URL 里丢精度**，别绕开它传裸数字。
- 纯数字 search 参数会被默认 JSON.parse 成 number——新路由照抄 `/detail` 的 `validateSearch` 兜底。
- 深链调试用 `node scripts/cdp.mjs nav <route>`（路由参数不带前导斜杠，MSYS 路径转换坑）。

## 样式

- Tailwind v4 CSS-first：设计令牌（oklch）在 `src/styles/index.css`，**没有 tailwind.config**；暗色靠 `<html class="dark">` 的 `@custom variant`。
- 组件内原子类 + `cn()`（clsx + tailwind-merge）；shadcn 件用 cva 变体。自定义全局类（`hg-loadbar`、`scrollbar-thin` 等）写在 index.css，不散落。
- 新颜色用现有令牌（`bg-background`、`text-muted-foreground`…），别写裸 hex——亮暗主题会漏。

## 前端测试

- vitest，** colocated `*.test.ts`，只测纯函数**（format/range/playback-prefs/danmaku-emoji 是范例）；无组件测试。
- UI 回归不写组件测试，走 CDP probe（`workflow.md` 调试节）——这是本仓库有意识的取舍，别引入 testing-library。
- 跑法：`pnpm test` / `pnpm test -- <文件名>`。

## 播放器专项（features/player）

最敏感区域：取流/转码兜底/进度持久化/剧终接力/悬浮层状态机/三套交互裁决全在 hooks（重构后）或 player-page.tsx（重构前）。改动原则：渲染结构不动、逻辑搬家不重写；自带竞态防御（带 key 读时校验、srcKey/cloudCounter 快照），**别当 redundancy 删掉**——每段都是修过的 bug。改完必冒烟：起播→切集→切清晰度→弹幕开关→小屏进出。
