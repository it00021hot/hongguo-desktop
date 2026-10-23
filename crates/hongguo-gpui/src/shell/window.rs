use super::*;

use gpui_kit::App;
use gpui_kit::component::WindowExt as _;

impl super::Shell {
    /// 老板键触发的播放处置（Tauri 前端 `boss-key-hidden` 事件同款）：
    /// 隐藏即暂停（声音不漏）、恢复时把暂停着的续上。窗口藏显由调用方在
    /// 实体更新之外做（见 `Shell::new` 的轮询注释）。
    pub(super) fn boss_key_playback(&self, hidden: bool, cx: &mut Context<Self>) {
        self.feed
            .update(cx, |home, cx| home.boss_key_hidden(hidden, cx));
    }

    /// 给 main 的启动回调：把窗口提到前台。
    pub fn activate(&self, cx: &mut App) {
        cx.activate(true);
    }

    /// 窗口关闭一律先问一句（Tauri `CloseRequested` → 前端确认框同款：自绘
    /// ×、Alt+F4、任务栏关闭都走这里；退出会掐断正在跑的下载任务）。
    /// 弹过还来（用户再点一次 ×）直接放行——宁可少问一句，绝不把窗口变成
    /// 关不掉的（Tauri 前端未就绪放行同精神）。
    /// 由 `main.rs` 在壳实体就位后挂上（`Shell::new` 里拿不到自身弱引用）。
    pub(crate) fn install_close_guard(&self, window: &mut Window, cx: &mut Context<Self>) {
        // 防抖落盘（Tauri `window_state::spawn_saver` 的 500ms 同款）：
        // macOS 的 Cmd+Q 走 NSApplication terminate，根本不发
        // `windowShouldClose`，只靠关闭回调存几何等于「退出就丢记忆」；
        // 强杀/崩溃同理。移动/缩放停下 500ms 内落一次盘，退出路径就不重要了。
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(GEOMETRY_SAVE_DEBOUNCE_MS))
                    .await;
                let Ok(dirty) = this.update(cx, |shell, _| std::mem::take(&mut shell.bounds_dirty))
                else {
                    break;
                };
                if dirty {
                    let _ = this.update(cx, |shell, _| shell.save_window_bounds());
                }
            }
        })
        .detach();

        let this = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            let Some(shell) = this.upgrade() else {
                return true; // 壳都没了：别拦
            };
            // 窗口几何随关闭保存（同步写：回调之后窗口可能立即销毁，tokio
            // 桥不保证还跑）；即使这次关闭被确认框拦下，存的也是当前真值
            shell.update(cx, |shell, _| shell.save_window_bounds());
            if shell.read(cx).exit_asked {
                log::info!("[Shell] 退出确认已弹过，本次关闭直接放行");
                return true;
            }
            // `Entity::update` 不会失败（实体在手）；弹窗就地在本次窗口
            // 回调里开（同一次 update 内，不再跨帧）
            shell.update(cx, |shell, cx| {
                shell.exit_asked = true;
                let view = cx.entity();
                window.open_alert_dialog(cx, move |dialog, _window, _cx| {
                    let view = view.clone();
                    let view_cancel = view.clone();
                    dialog
                        .title(crate::i18n::t("window.exitTitle"))
                        .description(crate::i18n::t("window.exitBody"))
                        .ok_text(crate::i18n::t("window.exitConfirm"))
                        .cancel_text(crate::i18n::t("common.cancel"))
                        // 组件库的取消钮默认不显示（show_cancel=false）：只
                        // 设文案不给按钮 = 确认框没有取消键（Tauri 确认框
                        // 是「取消 + 确认」两键，Esc 只是额外通道）
                        .show_cancel(true)
                        .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                        .on_ok(move |_, _window, cx| {
                            log::info!("[Shell] 用户在退出确认框里选了退出");
                            cx.quit();
                            true
                        })
                        // 取消/点背景关掉：把闸门放回未问状态，下次关闭
                        // 还会再问（Tauri 取消后对话框收起 = 下次照问）
                        .on_cancel(move |_, _window, cx| {
                            view_cancel.update(cx, |shell, cx| {
                                shell.exit_asked = false;
                                cx.notify();
                            });
                            true
                        })
                });
            });
            false // 拦下这次关闭，等确认框结果
        });
    }

    /// 把采样到的三槽窗口几何并进偏好同步落盘（读-改-写：只覆盖已采样
    /// 到的槽位——w 为 0 表示本会话没进过该形态，盘上记忆保持不动）。
    fn save_window_bounds(&self) {
        let mut prefs = crate::ui_prefs::load();
        if self.win_bounds[2] > 0.0 {
            let [x, y, w, h] = self.win_bounds;
            (prefs.win_x, prefs.win_y, prefs.win_w, prefs.win_h) = (x, y, w, h);
        }
        if self.mini_bounds[2] > 0.0 {
            let [x, y, w, h] = self.mini_bounds;
            (prefs.mini_x, prefs.mini_y, prefs.mini_w, prefs.mini_h) = (x, y, w, h);
        }
        if self.portrait_bounds[2] > 0.0 {
            let [x, y, w, h] = self.portrait_bounds;
            (
                prefs.portrait_x,
                prefs.portrait_y,
                prefs.portrait_w,
                prefs.portrait_h,
            ) = (x, y, w, h);
        }
        crate::ui_prefs::save_sync(&prefs);
    }
}
