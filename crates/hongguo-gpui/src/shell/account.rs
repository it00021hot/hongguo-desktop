use super::*;

/// 侧栏账户头像的显示边长（物理像素）：侧栏画的是 `size_5()`（1.25rem =
/// 20px）。头像按这个尺寸预缩进 GPU——渲染器没有 mipmap，原图直接缩到
/// 20px 画出来必然糊（见 [`crate::cover_img::decode`]）。
const SIDEBAR_AVATAR_PX: u32 = 20;

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
                    && let Some(avatar) = crate::cover_img::decode(&bytes, SIDEBAR_AVATAR_PX)
                {
                    shell.account_avatar = Some(avatar);
                    shell.account_avatar_url = Some(url);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
