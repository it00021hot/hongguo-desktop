//! gpui 资源加载的 HTTP 客户端：把远程封面请求桥到壳已有的 reqwest（与
//! hongguo-api 同一条 rustls 线路）；header 与状态码统一经字符串/u16 转换，
//! 避免 http 1.x 双版本的类型互斥。

use std::sync::Arc;

use anyhow::anyhow;
use futures::future::BoxFuture;
use gpui_kit::http_client::{AsyncBody, HttpClient, Request, Response};
use gpui_kit::{App, http_client as hc};

/// reqwest 背书的 [`HttpClient`] 实现，供 gpui 图片缓存拉远程封面。
pub struct ReqwestClient {
    client: reqwest::Client,
}

impl ReqwestClient {
    /// 构造并安装进 gpui 全局（封面 CDN 是签名 URL，UA 用浏览器形态）。
    pub fn install(cx: &mut App) {
        let mut builder = reqwest::Client::builder().user_agent("Mozilla/5.0");
        if hongguo_common::offline_qa::enabled() {
            builder = builder
                .no_proxy()
                .redirect(reqwest::redirect::Policy::custom(|attempt| {
                    if hongguo_common::offline_qa::parsed_url_forbidden(attempt.url()) {
                        attempt.error(std::io::Error::other("离线 QA 拒绝重定向到外部地址"))
                    } else {
                        attempt.follow()
                    }
                }));
        }
        let client = builder.build().expect("reqwest 客户端初始化失败");
        cx.set_http_client(Arc::new(Self { client }));
    }
}

impl HttpClient for ReqwestClient {
    fn user_agent(&self) -> Option<&hc::http::HeaderValue> {
        None
    }

    fn proxy(&self) -> Option<&hc::Url> {
        None
    }

    fn send(
        &self,
        req: Request<AsyncBody>,
    ) -> BoxFuture<'static, anyhow::Result<Response<AsyncBody>>> {
        let (parts, body) = req.into_parts();
        let method = parts.method.as_str().to_string();
        let url = parts.uri.to_string();
        let headers: Vec<(String, String)> = parts
            .headers
            .iter()
            .filter_map(|(k, v)| {
                v.to_str()
                    .ok()
                    .map(|v| (k.as_str().to_string(), v.to_string()))
            })
            .collect();
        // 协议路径上的图片请求都是 GET 空体；bytes 形态照搬，流式体不支持。
        let request_body = match body.0 {
            hc::Inner::Empty => None,
            hc::Inner::Bytes(cursor) => Some(cursor.into_inner()),
            hc::Inner::AsyncReader(_) => {
                return Box::pin(async { Err(anyhow!("gpui 桥不支持流式请求体")) });
            }
        };

        if hongguo_common::offline_qa::external_url_forbidden(&url) {
            return Box::pin(async { Err(anyhow!("离线 QA 拒绝外部请求")) });
        }
        let client = self.client.clone();
        // reqwest（hyper）的 DNS 解析需要 tokio 反应器；gpui 执行器线程上
        // 没有。转发到壳的常驻 tokio 运行时执行，oneshot 回传。
        let (tx, rx) = tokio::sync::oneshot::channel();
        crate::tokio().spawn(async move {
            let outcome: anyhow::Result<(u16, reqwest::header::HeaderMap, bytes::Bytes)> = async {
                let mut req = client.request(
                    reqwest::Method::from_bytes(method.as_bytes()).expect("HTTP 方法合法"),
                    &url,
                );
                for (k, v) in headers {
                    req = req.header(k, v);
                }
                if let Some(bytes) = request_body {
                    req = req.body(bytes);
                }
                let resp = req.send().await.map_err(|e| anyhow!("请求失败: {e}"))?;
                let status = resp.status().as_u16();
                let headers = resp.headers().clone();
                let bytes = resp.bytes().await.map_err(|e| anyhow!("读响应失败: {e}"))?;
                Ok((status, headers, bytes))
            }
            .await;
            let _ = tx.send(outcome);
        });
        Box::pin(async move {
            let (status, headers, bytes) = rx.await.expect("tokio 桥不会丢弃发送端")?;

            let mut builder = Response::builder().status(status);
            for (k, v) in headers.iter() {
                if let Ok(v) = v.to_str() {
                    builder = builder.header(k.as_str(), v);
                }
            }
            builder
                .body(AsyncBody::from_bytes(bytes))
                .map_err(|e| anyhow!("构造响应失败: {e}"))
        })
    }
}
