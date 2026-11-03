/**
 * 全部 IPC 调用统一出口（组件禁止直接 invoke）；参数名与 Rust 侧严格一致
 * （Tauri 把 JS 对象的键直接映射为 command 参数名）。
 */
export { app } from './app';
export { capability } from './capability';
export { settings } from './settings';
export { login } from './login';
export { series } from './series';
export { discover } from './discover';
export { danmaku } from './danmaku';
export { interact } from './interact';
export { emoticon } from './emoticon';
export { follow } from './follow';
export { rank } from './rank';
export { seriesSearch } from './search';
export { download } from './download';
export { merge } from './merge';
export { play } from './play';
export { watchHistory } from './watch-history';
export { transcode } from './transcode';
export { storage } from './storage';
