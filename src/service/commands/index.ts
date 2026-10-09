/**
 * 全部 IPC 调用集中在这里（按域拆分，此处统一出口）。
 *
 * 组件不要直接 `invoke`——统一走这里才能保证 zod 校验与错误归一，
 * 也让「后端改了 command 签名」这件事在类型检查时立刻暴露。
 *
 * 参数名与 Rust 侧严格一致（Tauri 会把 JS 对象的键直接映射为 command 参数名）。
 */
export { app } from './app';
export { settings } from './settings';
export { login } from './login';
export { series } from './series';
export { discover } from './discover';
export { danmaku } from './danmaku';
export { interact } from './interact';
export { rank } from './rank';
export { seriesSearch } from './search';
export { download } from './download';
export { merge } from './merge';
export { play } from './play';
export { watchHistory } from './watch-history';
export { transcode } from './transcode';
export { storage } from './storage';
