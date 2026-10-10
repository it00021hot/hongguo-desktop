/**
 * 播放器内部的瞬时信号（模块级时间戳）：跨一次组件重挂载传递的毫秒级
 * 会话态，不值得进 zustand。两个信号都是「事件发生在 A 实例、消费在 B 实例」——
 * PlayerView 按 key 随切集整体重建，ref 传不过去，模块级是唯一通道。
 */

/** 自动连播（handleEnded）切换的时间戳。 */
let autoAdvanceAt = 0;
/** 选集浮层关闭的时间戳。 */
let pickerClosedAt = 0;

/** 自动连播切集时打点：新挂载的 PlayerView 据此以「无人操作」静默起步。 */
export function markAutoAdvance(): void {
  autoAdvanceAt = Date.now();
}

/**
 * 挂载时问一次：这次挂载是不是自动连播带来的。带 5 秒时效——信息流上下文
 * 里 PlayerView 不随切剧重挂载，标记可能残留到很久以后的某次挂载，过期作废。
 */
export function isAutoAdvanceStart(): boolean {
  return Date.now() - autoAdvanceAt < 5_000;
}

/** 选集浮层任何路径关闭时打点（选中/Esc/收起按钮）。 */
export function markPickerClosed(): void {
  pickerClosedAt = Date.now();
}

/**
 * 选集浮层刚关闭的一瞬，跟手/连击的点击会落到舞台上——浮层已卸载，
 * data-wheel-block 拦不住，就成了一次无意识的播放/暂停切换。短窗内忽略。
 */
export function isPickerClickCooldown(): boolean {
  return Date.now() - pickerClosedAt < 400;
}
