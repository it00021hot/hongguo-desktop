use super::*;

use std::sync::Arc;

impl super::Shell {
    /// 账号变化（登录/登出）：头像重拉 + 追更角标重算。
    pub(super) fn on_account_changed(&mut self, cx: &mut Context<Self>) {
        self.account_avatar_url = None;
        self.ensure_account_avatar(cx);
        self.refresh_follow_unread(cx);
    }

    /// 账号头像加载：账号在且 URL 没拉过才拉一次（登录/换头像后重拉，
    /// 登出即清）；下载失败留空退回通用图标。
    pub(super) fn ensure_account_avatar(&mut self, cx: &mut Context<Self>) {
        let Some(account) = self.state.settings().account else {
            self.account_avatar = None;
            self.account_avatar_url = None;
            return;
        };
        let url = account.avatar_url;
        if url.is_empty() || self.account_avatar_url.as_deref() == Some(url.as_str()) {
            return;
        }
        if self.account_avatar_pending {
            return;
        }
        self.account_avatar_pending = true;
        let rx = crate::cover_img::fetch(url.clone());
        cx.spawn(async move |this, cx| {
            let bytes = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |shell, cx| {
                shell.account_avatar_pending = false;
                if let Ok(bytes) = bytes
                    && let Ok(decoded) = image::load_from_memory(&bytes)
                {
                    shell.account_avatar = Some(Arc::new(crate::image_upload::render_image(
                        decoded.to_rgba8(),
                    )));
                    shell.account_avatar_url = Some(url);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
