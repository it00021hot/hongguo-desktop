//! 登录服务：MFA 后台轮询、会话凭据处理、账号落库与资料刷新；
//! command 层只做参数校验与返回模型组装，登录编排全在这一层。

pub mod account;
pub mod csrf;
pub mod events;
pub mod flow;
pub mod polling;
