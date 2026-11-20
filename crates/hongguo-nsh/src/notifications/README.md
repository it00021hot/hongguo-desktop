# 系统更新通知 / System notifications

通知使用 NSH 0.6.1 的 `request_notification_permission`、`show_notification` 和
`next_notification_press`；权限、原生通知中心、投递与回执由框架管理。
不再注册自定义通知组件，也不维护请求队列或直接调用平台通知 API。

`Notifications` 捕获 App owner。跨页面的异步任务在该 owner 下发起框架请求，
页面销毁不取消已开始的通知操作。点击通知后使用 NSH `WindowRef::focus` 激活当前窗口。
追更消息继续逐条等待发送结果；系统接受请求不等于用户已经看到通知。

权限仅在用户开启「弹出提醒」时申请。关闭开关不申请权限、不发送通知；
本地提醒记录仍可查看。权限拒绝或系统不支持时返回未授权，发送失败继续报告错误。

桌面宿主由 NSH 启用系统通知。iOS 使用 SDK 的通知生命周期；Android 使用 SDK 的
通知权限处理与通知频道，Activity 在 `onCreate` 和 `onNewIntent` 中调用
`NshView.handleNotificationIntent`，接回冷启动和已运行应用的通知点击。
Android manifest 保留 `POST_NOTIFICATIONS`。不再保留应用自建的 `NotificationComponent`。

这是本地通知，消息仍来自追更检查服务；没有 APNs/FCM 或应用自建后台服务。
手机进程被挂起或终止时，不承诺继续检查更新。

本轮验证以 Windows 构建、NSH 检查和严格 Clippy 为准；系统授权、实际通知展示及
点击激活仍需对应平台运行验收。旧通知桥的验证记录不代表当前 NSH 实现已经实测通过。

The app uses NSH notification permission, delivery and press APIs under the application owner.
Follow-update messages remain sequential, notification presses focus the current NSH window,
and Android forwards both initial and subsequent intents to the embedding SDK.
No custom notification component, transport queue or direct notification binding remains.
Platform permission dialogs, notification presentation and activation require runtime acceptance.
