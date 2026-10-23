use super::*;

use gpui_kit::SharedString;
use hongguo_service::events::ServiceEvent;

impl super::Shell {
    /// 壳级服务事件消费（独立通道；与 HomeView 的事件循环互不影响）。
    pub(super) fn on_service_event(&mut self, event: ServiceEvent, cx: &mut Context<Self>) {
        if let ServiceEvent::FollowNotices(payload) = &event {
            if crate::ui_prefs::load().aibot_popup_on {
                for notice in &payload.notices {
                    cx.show_system_notification(gpui_kit::SystemNotification {
                        tag: SharedString::from(format!("follow:{}", notice.id)),
                        title: SharedString::from(notice.title.clone()),
                        body: SharedString::from(notice.detail.clone()),
                        actions: Vec::new(),
                    });
                }
            }
            // 新提醒：角标刷新 + 追更页列表翻面（数据在本地库，一次小读；
            // 页面在不在前台都刷新，回到页面即最新）
            self.refresh_follow_unread(cx);
            self.follows
                .update(cx, |view, cx| view.on_notices_arrived(cx));
        }
    }
}
