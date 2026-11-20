//! LiteMiyu 的纯逻辑（`docs/00-设计.md` 第四节第 4 条）：不做 I/O、不读时钟，没有 I/O 的依赖，编译器守着。
//!
//! 从 Miyu 搬来的，一个模块原来是它的一个 crate：
//!
//! - [`kernel`]：内核。事件、回合的状态机、投影、压缩（Miyu 的 `miyu-kernel`）；
//! - [`drivers`]：三种模型接口的编解码，纯翻译（Miyu 的 `miyu-drivers`）；
//! - [`assemble`]：把一段有效历史组装成一份请求（Miyu 的 `miyu-assemble`）；
//! - [`config`]：配置的清单、读法、校验、改法，纯的，不碰文件（Miyu 的 `miyu-config`）；
//! - [`policy`]：策略。给模型看的字怎么拼、策略快照、高危命令的认法（Miyu 的 `miyu-policy`）；
//! - [`models`]：模型这一块：供应商的档案、池、冷却、models.dev 的目录怎么读、模型的资料（Miyu 的 `miyu-models`）。

pub mod assemble;
pub mod config;
pub mod drivers;
pub mod kernel;
pub mod models;
pub mod policy;
