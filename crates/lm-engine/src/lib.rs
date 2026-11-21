//! LiteMiyu 的 I/O（`docs/00-设计.md` 第四节第 4 条）：`lm-kernel` 是纯逻辑，碰磁盘、网络、时钟的都在这里。
//!
//! 现在有的：
//!
//! - [`fs`]：工具怎么碰文件：路径换成真实的位置、边界、安全地打开、整体换掉（Miyu 的 `miyu-fs`）；
//! - [`store`]：数据放哪、会话日志、blob、配置文件、密钥（Miyu 的 `miyu-store` 里留下的）；
//! - [`configured`]：照配置文件取会话的配置（Miyu 核心的配置服务里会话要的那一半）；
//! - [`zone`]：本机时区，照 `/etc/localtime` 读（不走 zoneinfo 整个目录），主程序也用它；
//! - [`http`]：发请求：照驱动编码好的字节发，流式读回来交给解码器（Miyu 的 `miyu-http`）；
//! - [`models`]：编进程序的模型目录快照，用到时才解开；
//! - [`tool`]：工具的接口、目录、各个端口（Miyu 的 `miyu-tool`）；
//! - [`schedule`]：循环的工具 `schedule_wakeup`、`/loop` 后面的字怎么读（Miyu 的 `miyu-schedule` 里循环那一半）；
//! - [`skills`]：技能：找、开关、列进 system 的那一块、自带的 `skill-creator`（Miyu `miyu-store`、`miyu-session` 里技能那几块）。
//! - [`mcp`]：MCP 的客户端：认时代、列工具、调工具，标准输入输出和网址两种连法（Miyu 的 `miyu-mcp` 里不带登录的那一半）。
//! - [`presets`]：预设：配置里一张张带名字的开关名单，管内置工具、MCP 服务、技能（LM-29）。
//! - [`checkup`]：被忽略的配置项、MCP 服务、技能，引擎本来要读的时候顺手记下（运行日志去掉以后它们没处说，主程序在第一句话以后汇成一条提示）；
//! - [`providers`]：供应商的增删改、试通不通、从 Miyu 导入，不带界面的那一层（命令行调，配置界面以后也调它）。

pub mod basesystem;
pub mod checkup;
pub mod configured;
pub mod fs;
pub mod http;
pub mod mcp;
pub mod models;
pub mod presets;
pub mod providers;
pub mod resources;
pub mod schedule;
pub mod session;
pub mod settings;
pub mod skills;
pub mod store;
pub mod tool;
pub mod zone;
