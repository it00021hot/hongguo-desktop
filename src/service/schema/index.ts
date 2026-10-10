/**
 * 与 Rust serde 模型对应的 zod schema，按域拆分的统一出口。
 *
 * 这里是**唯一**的手写契约副本：Rust 侧 `domain/model/*` 改了字段，
 * 这里必须同步改，否则运行时才报错。类型检查的价值就在于此。
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
export * from './interact';
export * from './rank';
export * from './search';
