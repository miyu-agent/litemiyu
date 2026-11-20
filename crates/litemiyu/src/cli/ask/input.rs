//! 要说的话从哪来（照 Miyu `cli/ask.md`「标准输入」）：命令行上的几个词、标准输入。
//!
//! 1. 没写消息、标准输入不是终端：读到头，整段就是消息。
//! 2. 有一个词正好是 `-`：去掉它，读标准输入接在其余几个词后面，中间空一行；其余是空的整段就是消息；读到空白的只用写的。
//! 3. 写了消息、没写 `-`：不读（拉起它的程序常常留着一个不关的管道，一看到管道就读会一直等）。
//! 4. 标准输入是终端、又没写消息：缺少消息，不等人敲。

use std::io::{IsTerminal, Read};

/// 凑出来的话最长多少字节：读到这么多还没完的不往下读。
pub const LIMIT: usize = 8 << 20;

/// 凑不出话。
#[derive(Debug, PartialEq, Eq)]
pub enum NoMessage {
    /// 没写、标准输入是终端，或者凑出来是空的、只有空白。
    Missing,
    /// 标准输入不是 UTF-8。
    NotUtf8,
    /// 超过 [`LIMIT`]。
    TooLong,
    /// 读的时候出错：系统的原话。
    Unreadable(String),
}

/// 照写的几个词 `words` 和标准输入凑出要说的话。
///
/// # Errors
///
/// 见 [`NoMessage`]。
pub fn message(words: &[String]) -> Result<String, NoMessage> {
    let stdin = std::io::stdin();
    let terminal = stdin.is_terminal();
    compose(words, terminal, || read_limited(&mut stdin.lock()))
}

/// 凑：`read` 读标准输入（要读时才调）。
fn compose(
    words: &[String],
    terminal: bool,
    read: impl FnOnce() -> Result<String, NoMessage>,
) -> Result<String, NoMessage> {
    let dash = words.iter().any(|word| word == "-");
    let typed: Vec<&str> = words
        .iter()
        .filter(|word| *word != "-")
        .map(String::as_str)
        .collect();
    let typed = typed.join(" ");
    let message = match (dash, typed.is_empty()) {
        (true, _) => {
            let piped = read()?;
            match (typed.is_empty(), piped.trim().is_empty()) {
                (true, _) => piped,
                (false, true) => typed,
                (false, false) => format!("{typed}\n\n{piped}"),
            }
        }
        (false, true) if !terminal => read()?,
        (false, _) => typed,
    };
    if message.trim().is_empty() {
        return Err(NoMessage::Missing);
    }
    if message.len() > LIMIT {
        return Err(NoMessage::TooLong);
    }
    Ok(message)
}

/// 读到头，最多 [`LIMIT`] 加一个字节（多出来的那一个说明超了）。
fn read_limited(from: &mut impl Read) -> Result<String, NoMessage> {
    let mut bytes = Vec::new();
    from.take(LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| NoMessage::Unreadable(error.to_string()))?;
    if bytes.len() > LIMIT {
        return Err(NoMessage::TooLong);
    }
    String::from_utf8(bytes).map_err(|_| NoMessage::NotUtf8)
}

#[cfg(test)]
mod tests {
    use super::{LIMIT, NoMessage, compose, read_limited};

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    fn piped(text: &'static str) -> impl FnOnce() -> Result<String, NoMessage> {
        move || Ok(text.to_string())
    }

    #[test]
    fn words_are_joined_and_a_dash_appends_the_pipe_after_a_blank_line() {
        let never = || -> Result<String, NoMessage> { panic!("不该读标准输入") };
        assert_eq!(
            compose(&words(&["审", "一下"]), false, never),
            Ok("审 一下".into())
        );
        assert_eq!(
            compose(&words(&["-", "审一下"]), false, piped("diff\n")),
            Ok("审一下\n\ndiff\n".into()),
            "`-` 写在哪都接在后面"
        );
        assert_eq!(
            compose(&words(&["-"]), true, piped("全是它")),
            Ok("全是它".into())
        );
        assert_eq!(
            compose(&words(&["审", "-"]), false, piped("  \n")),
            Ok("审".into())
        );
        assert_eq!(compose(&[], false, piped("整段\n")), Ok("整段\n".into()));
    }

    #[test]
    fn a_terminal_without_words_and_blank_input_are_missing() {
        let never = || -> Result<String, NoMessage> { panic!("终端不等人敲") };
        assert_eq!(compose(&[], true, never), Err(NoMessage::Missing));
        assert_eq!(compose(&[], false, piped(" \n")), Err(NoMessage::Missing));
    }

    #[test]
    fn too_long_and_not_utf8_are_refused() {
        let big = vec![b'a'; LIMIT + 10];
        assert_eq!(read_limited(&mut big.as_slice()), Err(NoMessage::TooLong));
        let exact = vec![b'a'; LIMIT];
        assert_eq!(
            read_limited(&mut exact.as_slice()).map(|s| s.len()),
            Ok(LIMIT)
        );
        assert_eq!(
            read_limited(&mut &[0xff, 0xfe][..]),
            Err(NoMessage::NotUtf8)
        );
    }
}
