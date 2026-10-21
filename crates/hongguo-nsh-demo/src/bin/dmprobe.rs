//! 一次性探测：单测弹幕拉取（与 UI 无关，定位 dm.await 挂住问题）。

use hongguo_service::app_state::{AppState, AppStateInner};

fn main() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();
    let state: AppState = std::sync::Arc::new(AppStateInner::default());
    hongguo_service::runtime::runtime().block_on(async move {
        let env = state.api_env();
        let t0 = std::time::Instant::now();
        let all = hongguo_api::danmaku::fetch_danmaku_all(
            "7693121384184024089",
            "7693111859401985049",
            &env,
        )
        .await;
        match all {
            Ok(list) => {
                println!(
                    "弹幕 {} 条，耗时 {:.1}s",
                    list.len(),
                    t0.elapsed().as_secs_f32()
                );
                for d in list.iter().take(6) {
                    println!("  [{:.1}s] {}", d.offset_ms as f32 / 1000.0, d.text);
                }
            }
            Err(e) => println!("拉取失败: {e}"),
        }
        // 对照组：同 vid 的评论区（口径已另行锁定）。评论有、弹幕只回
        // 引导条 ⇒ 设备/风控门控；评论也空 ⇒ 服务端口径漂移。
        let t1 = std::time::Instant::now();
        match hongguo_api::danmaku::fetch_comments_page(
            "7693121384184024089",
            "7693111859401985049",
            "",
            &env,
        )
        .await
        {
            Ok(page) => {
                println!(
                    "评论 {} 条（总数 {}），耗时 {:.1}s",
                    page.items.len(),
                    page.total,
                    t1.elapsed().as_secs_f32()
                );
                for c in page.items.iter().take(4) {
                    println!("  [{}] {}", c.user_name, c.text);
                }
            }
            Err(e) => println!("评论拉取失败: {e}"),
        }
    });
}
