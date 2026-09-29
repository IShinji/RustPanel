//! 面向用户的 RPC 提示按请求语言生成。机制:请求头 + `tokio::task_local!`,
//! 不改 proto、不改任何 handler 签名——唯一接入点是 `lib.rs` 的
//! `multiplex_service_with_auth` 里那个 `service_fn` 闭包(所有 gRPC / HTTP
//! 请求都过这一个函数),在那里读 `x-rustpanel-locale` 请求头后把整个请求
//! 包进 [`scope`]。
//!
//! 已知局限:`task_local` 值不会带进 `tokio::spawn` / `spawn_blocking`,
//! 以及部分 streaming RPC 在 handler 返回之后才 poll 的部分——这些地方会
//! 退回默认语言(简体中文),这是当前已知行为,不算回归。
//!
//! 分诊原则:只有用户能看懂并可能据此操作的消息(成功提示、
//! invalid_argument / already_exists / failed_precondition / not_found)才
//! 双语化;纯内部诊断信息直接用英文即可。

use std::future::Future;

use axum::http::HeaderMap;

/// 请求语言;默认简体中文,与迁移前的行为一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Locale {
    #[default]
    ZhCn,
    En,
}

impl Locale {
    fn from_header_value(value: &str) -> Self {
        if value.trim().eq_ignore_ascii_case("en") {
            Locale::En
        } else {
            Locale::ZhCn
        }
    }
}

/// 前端 `lib/rpc.ts` 的 `authInterceptor` / `authFetch` 都会带上这个请求头。
pub const LOCALE_HEADER: &str = "x-rustpanel-locale";

tokio::task_local! {
    static LOCALE: Locale;
}

/// 当前 task 的语言;拿不到(没有 scope 过,或值已经离开 task_local 作用域)
/// 时退回默认(简体中文)。
pub fn current() -> Locale {
    LOCALE.try_with(|locale| *locale).unwrap_or_default()
}

/// 双语文案二选一,不需要插值时比 `trf!` 宏更轻量。
pub fn tr(zh: &'static str, en: &'static str) -> &'static str {
    match current() {
        Locale::ZhCn => zh,
        Locale::En => en,
    }
}

/// 从请求头解析语言;读不到或值不认识一律回落默认语言。
pub fn from_headers(headers: &HeaderMap) -> Locale {
    match headers
        .get(LOCALE_HEADER)
        .and_then(|value| value.to_str().ok())
    {
        Some(value) => Locale::from_header_value(value),
        None => Locale::default(),
    }
}

/// 把 `future` 包进给定语言的 task_local 作用域跑;`multiplex_service_with_auth`
/// 的 `service_fn` 闭包用它包住每个请求的处理逻辑。
pub async fn scope<F: Future>(locale: Locale, future: F) -> F::Output {
    LOCALE.scope(locale, future).await
}

/// 双语 + `format!` 插值。按当前语言选中英文模板之一,再各自插值——
/// 因为两种语言的参数顺序 / 措辞可能不同,不能只翻译模板字符串本身。
/// 参数按原始 token 转发给 `format!`,所以位置参数(`name`)和具名参数
/// (`name = value`)都支持,和直接写 `format!` 的写法一致。
///
/// ```ignore
/// let msg = trf!("站点 {} 已创建", "site {} created", name);
/// let msg = trf!("站点 {name} 已创建", "site {name} created", name = name);
/// ```
#[macro_export]
macro_rules! trf {
    ($zh:literal, $en:literal $(, $($arg:tt)+)?) => {
        match $crate::i18n::current() {
            $crate::i18n::Locale::ZhCn => format!($zh $(, $($arg)+)?),
            $crate::i18n::Locale::En => format!($en $(, $($arg)+)?),
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_headers_reads_en_case_insensitively() {
        let mut headers = HeaderMap::new();
        headers.insert(LOCALE_HEADER, "EN".parse().unwrap());
        assert_eq!(from_headers(&headers), Locale::En);
    }

    #[test]
    fn from_headers_defaults_to_zh_cn_when_missing_or_unknown() {
        assert_eq!(from_headers(&HeaderMap::new()), Locale::ZhCn);

        let mut headers = HeaderMap::new();
        headers.insert(LOCALE_HEADER, "fr".parse().unwrap());
        assert_eq!(from_headers(&headers), Locale::ZhCn);
    }

    #[tokio::test]
    async fn current_reflects_the_active_scope() {
        assert_eq!(current(), Locale::ZhCn);
        let seen = scope(Locale::En, async { current() }).await;
        assert_eq!(seen, Locale::En);
        // scope 结束后回落默认值,不会泄漏到外层 task。
        assert_eq!(current(), Locale::ZhCn);
    }

    #[tokio::test]
    async fn tr_and_trf_switch_with_the_active_scope() {
        let (plain, interpolated) = scope(Locale::En, async {
            (
                tr("你好", "hello"),
                trf!("站点 {} 已创建", "site {} created", "demo"),
            )
        })
        .await;
        assert_eq!(plain, "hello");
        assert_eq!(interpolated, "site demo created");
    }
}
