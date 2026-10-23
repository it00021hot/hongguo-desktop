//! `home/season.rs` 的单测宿主：源文件级引入（壳是纯 bin、无 lib 目标，
//! 见 `preview_scenes.rs` 的同款说明）。断言全部写在 season.rs 内。

#[path = "../src/home/season.rs"]
mod season;
