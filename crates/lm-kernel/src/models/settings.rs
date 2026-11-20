//! 模型这一块的配置项（`docs/blueprint/models.md`「配置：模型这一块的键」，`config.md`「配置清单」，施工 8-6）：
//! 一家供应商 `[providers.<id>]`、一个模型手写的资料 `[providers.<id>.models."<model>"]`、用途 `[models]`。（LM-8 砍了模型池：
//! 池 `[pools.<名字>]`、冷却 `[models.cooldown]`、供应商的缓存类别 `cache` 记成退役的键，`crate::config::retired`。）
//!
//! 8-6 只声明用得上的几格：驱动、地址、key（8-25 起一家一个，原来是几个）、对应目录里的哪一家、模型的窗口、主对话的模型。8-7 加上模型资料要的
//! （`models.md`「模型的资料」）：供应商的倍率、本机；模型手写的资料（对目录里的哪一个、最大输出、能收什么、能不能调工具、
//! 思考强度、价格、倍率）；目录怎么更新 `[models.catalog]`。8-8 加上看图的模型（池、供应商的缓存类别、8-9 的冷却 LM-8 砍掉）。
//! 别的格（另配的头、开关、占位工具、模型的驱动）随用到它的那一步加（「施工时定的」8-6、8-7、8-8）。8-18 加上模型默认的思考强度
//! `effort`。8-21 加上供应商的显示名 `name`。8-22 加上模型默认的温度 `temperature`。项目配置一项都不能写。
//!
//! `base_url` 8-6b 起也能写 `{ env = … }`：地址不进任何回应、日志、文件，照核心起来时的环境取（[`crate::models::provider`] 的
//! `resolve_base_url`）。

use std::time::Duration;

use crate::config::secret::Reference;
use crate::config::{Address, Number};

crate::settings! {
    /// 一家供应商（`models.md`「怎么走」第一条）：编号是键里 `<id>` 那一段，「路径里的名字」的写法。
    pub struct ProviderSettings in "providers.<id>" {
        /// 界面上给人看的名字（施工 8-21）：编号只用来引用模型。不写的照目录里对上的那一家的名字，再没有的照编号
        /// （`model.list` 的 `name`）。只给界面看，不进请求，当场生效。
        name: Option<String> = none {
            kind: text [64],
            layers: [System],
            applies: now,
            ui: { page: "models", group: "providers", control: text },
        },
        /// 怎么说话。不写照档案推；推不出来的这一家用不了。
        driver: Option<String> = none {
            kind: option ["openai-chat", "anthropic", "openai-responses"],
            layers: [System],
            applies: next_turn,
            ui: { page: "models", group: "providers", control: select },
        },
        /// 地址，路径由驱动接在后面。不写照档案推。可以是写死的，也可以是一个环境变量的引用（`{ env = … }`，施工
        /// 8-6b）：本机端点地址和 key 一样，只想放在拉起核心的环境变量里，不进任何文件。
        base_url: Option<Address> = none {
            kind: url,
            layers: [System],
            applies: next_turn,
            ui: { page: "models", group: "providers", control: text },
        },
        /// key：`{ secret = … }` 或 `{ env = … }`。一家一个（施工 8-25，`15-模型与供应商.md` M10）。不写的不带认证头。
        key: Option<Reference> = none {
            kind: secret,
            layers: [System],
            applies: next_turn,
            ui: { page: "models", group: "providers", control: text },
        },
        /// 手写指定这一家对应目录里的哪一家：照它找档案、认目录（`models.md`「怎么走」第二条第 4 条第 2 层）。
        catalog: Option<String> = none {
            kind: name,
            layers: [System],
            applies: next_turn,
            ui: { page: "models", group: "providers", control: text },
        },
        /// 本机的模型服务：价格当 0（第二条第 12 条，施工 8-7）；第一次接入时也照它找。不写的照地址：在本机的是。Miyu 这一层还有
        /// 倍率 `price_multiplier`，LiteMiyu 没登记（主程序从 Miyu 导入供应商时照登记的抄，它的测试守着不抄这一项：
        /// `settings/import/tests.rs`），倍率只写在模型上。
        local: Option<bool> = none {
            kind: bool,
            layers: [System],
            applies: next_turn,
            ui: { page: "models", group: "providers", control: toggle },
        },
    }
}

crate::settings! {
    /// 一个模型手写的资料（`models.md`「对外的样子」）：模型名是键里 `<model>` 那一段。每一格都压过用出来的、供应商的列表、
    /// 目录（「模型的资料」那张表）。窗口、最大输出造会话、载入时交给内核，开着的会话下一个回合开始时跟着换（施工 8-10）。
    pub struct ModelSettings in "providers.<id>.models.<model>" {
        /// 上下文窗口，单位 token。
        window: Option<i64> = none {
            kind: int [1, 100000000],
            layers: [System],
            applies: next_turn,
            ui: { page: "models", group: "providers", control: number },
        },
        /// 手写指定照目录里的哪一个：`<目录里的供应商>/<目录里的模型>`（第二条第 4 条第 1 层，施工 8-7）。
        catalog: Option<String> = none {
            kind: text [256],
            layers: [System],
            applies: next_turn,
            ui: { page: "models", group: "providers", control: text },
        },
        /// 最大输出，单位 token（施工 8-7）。
        max_output: Option<i64> = none {
            kind: int [1, 100000000],
            layers: [System],
            applies: next_turn,
            ui: { page: "models", group: "providers", control: number },
        },
        /// 能收哪些输入（施工 8-7；音频、视频施工 8-27，2026-10-07 项目主人定：只是声明能收，驱动照旧不发）。
        inputs: Option<Vec<String>> = none {
            kind: options ["text", "image", "pdf", "audio", "video"],
            layers: [System],
            applies: next_turn,
            ui: { page: "models", group: "providers", control: list },
        },
        /// 能不能调工具（施工 8-7）：只给 `model.list` 看。
        tools: Option<bool> = none {
            kind: bool,
            layers: [System],
            applies: next_turn,
            ui: { page: "models", group: "providers", control: toggle },
        },
        /// 思考强度有哪几档（施工 8-7）：盖过目录的；`none`、`disabled` 读成 `off`（施工 8-18，[`crate::models::effort`]）。
        reasoning: Option<Vec<String>> = none {
            kind: texts [32],
            layers: [System],
            applies: next_turn,
            ui: { page: "models", group: "providers", control: list },
        },
        /// 默认的思考强度（施工 8-18，`models.md`「怎么走」第十一条第 2 条）：这个模型的一档，不写的请求里不带、照供应商的
        /// 默认。不在这时的档位里的照没写，配置报 `unknown_effort`（[`crate::models::effort::unknown`]）。
        effort: Option<String> = none {
            kind: text [32],
            layers: [System],
            applies: next_turn,
            ui: { page: "models", group: "providers", control: text },
        },
        /// 默认的温度（施工 8-22，`models.md`「怎么走」第十四条）：不写的请求里不带、照供应商的默认。这个模型用不了的
        /// （目录说不收温度、超过驱动的上限）照没写，配置报 `unusable_temperature`（[`crate::models::temperature::unusable`]）。
        temperature: Option<Number> = none {
            kind: float [0, 2],
            layers: [System],
            applies: next_turn,
            ui: { page: "models", group: "providers", control: number },
        },
        /// 倍率：价格照它乘（施工 8-7）。不写是 1。
        price_multiplier: Option<Number> = none {
            kind: float [0, 1000],
            layers: [System],
            applies: next_turn,
            ui: { page: "models", group: "providers", control: number },
        },
    }
}

crate::settings! {
    /// 一个模型手写的价格（施工 8-7）：每一百万 token 的价。写了一格就整份用手写的，不和目录的拼（「模型的资料」第一条）。
    pub struct PriceSettings in "providers.<id>.models.<model>.price" {
        /// 输入。
        input: Option<Number> = none {
            kind: float [0, 1000000],
            layers: [System],
            applies: next_turn,
            ui: { page: "models", group: "providers", control: number },
        },
        /// 输出。
        output: Option<Number> = none {
            kind: float [0, 1000000],
            layers: [System],
            applies: next_turn,
            ui: { page: "models", group: "providers", control: number },
        },
        /// 读缓存。
        cache_read: Option<Number> = none {
            kind: float [0, 1000000],
            layers: [System],
            applies: next_turn,
            ui: { page: "models", group: "providers", control: number },
        },
        /// 写缓存。
        cache_write: Option<Number> = none {
            kind: float [0, 1000000],
            layers: [System],
            applies: next_turn,
            ui: { page: "models", group: "providers", control: number },
        },
        /// 币种：ISO 4217 的三个字母。不写是 `USD`。
        currency: Option<String> = none {
            kind: text [3],
            layers: [System],
            applies: next_turn,
            ui: { page: "models", group: "providers", control: text },
        },
    }
}

crate::settings! {
    /// models.dev 的目录怎么更新（`models.md`「对外的样子」`[models.catalog]`、「怎么走」第二条第 3 条，施工 8-7）：当场生效。
    pub struct CatalogSettings in "models.catalog" {
        /// 去取、后台更新（LM-9 起程序不带目录：第一次要用到时取）。关掉只用缓存里已有的，没有的就没有目录。环境变量
        /// `LITEMIYU_CATALOG_UPDATE` 压过（离线的机器、测试拉起的程序）。
        update: bool = true {
            kind: bool,
            layers: [System],
            env: "LITEMIYU_CATALOG_UPDATE",
            applies: now,
            ui: { page: "models", group: "catalog", control: toggle },
        },
        /// 从哪取。也能写 `{ env = … }`（施工 8-8：网址类型整体认引用，8-6b 留下的这一项以前读成空的），照核心的环境取。
        url: Address = "https://models.dev/api.json" {
            kind: url,
            layers: [System],
            applies: now,
            ui: { page: "models", group: "catalog", control: text },
        },
        /// 缓存旧过这么久才再取。
        every: Duration = "24h" {
            kind: duration [3600, 2592000],
            layers: [System],
            applies: now,
            ui: { page: "models", group: "catalog", control: text },
        },
    }
}

crate::settings! {
    /// 用途（`models.md`「对外的样子」`[models]`）。
    pub struct UseSettings in "models" {
        /// 新会话默认用的模型：`<供应商>/<模型>`。没配的请求都是 `no_model`。
        chat: Option<String> = none {
            kind: reference,
            layers: [System],
            applies: new_session,
            ui: { page: "models", group: "uses", common: true, control: text },
        },
        /// 替看不了图的模型看图的模型（施工 8-8 只读进来、`model.list` 列出来；替看图随 8-17）。
        vision: Option<String> = none {
            kind: reference,
            layers: [System],
            applies: next_turn,
            ui: { page: "models", group: "uses", control: text },
        },
    }
}
