//! 模型的一次请求怎么收场（随机测试的执行器替身）：多半正常说完，四回里有一回出错，照几类抽。摘要请求报超长另用一串随机数
//! （`compacting.rs` 的 `some_overflow`）。

use super::*;

/// 说完了的结局：四回里有一回出错，出错的带着供应商说的要等多久（施工 3-5 下）。多半是可以重试的 503；也有限速的（等 3 秒，
/// 或者 10 分钟：太久不等）、认证失败的（不重试）。原来还有端口说换了端点的两种、全在冷却的两种（施工 8-9），LM-8 砍了模型池
/// 以后端口不再这么说：换成重试的走法和原来一样的（换了端点的认证失败当场再来 → 可重试的 503；换了端点的限速、全在冷却的 →
/// 同样等多久的限速），随机数照旧取，后面那串输入不跟着错开，三百例里照旧每条路都走得到。旧日志里的 `cooling` 不再来，另在
/// `scenario/models.rs` 查。
pub(super) fn some_ending(rng: &mut Rng) -> (Option<CallError>, Option<u64>) {
    if rng.below(4) > 0 {
        return (None, None);
    }
    let (class, message, wait) = match rng.below(16) {
        0 => (ErrorClass::Auth, "401", None),
        1 | 4 | 5 => (ErrorClass::RateLimited, "429", Some(3000)),
        2 | 6 => (ErrorClass::RateLimited, "429", Some(600_000)),
        _ => (ErrorClass::Retryable, "503", None),
    };
    let error = CallError {
        class,
        message: message.to_string(),
        status: None,
        retry_after_ms: None,
    };
    (Some(error), wait)
}
