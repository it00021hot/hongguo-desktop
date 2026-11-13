/**
 * schema 统一出口。唯一的手写契约副本：Rust 侧 `domain/model/*` 改了字段
 * 这里必须同步改，否则运行时才报错。
 */
export * from './series';
export * from './download';
export * from './settings';
export * from './login';
export * from './merge';
export * from './play';
export * from './history';
export * from './error';
export * from './feed';
export * from './detail';
export * from './danmaku';
export * from './follow';
export * from './interact';
export * from './emoticon';
export * from './rank';
export * from './search';
export * from './device';
export * from './window';
