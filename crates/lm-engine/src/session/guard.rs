//! 权限策略：执行前那条链，模块编号 `permissions`。LiteMiyu 只有完全放开（设计第五节第 6 条，2026-11-11 项目主人定），
//! 平常的工具调用不问人，只留三条拦的：
//!
//! 1. 灭顶的命令（删根、删家目录、格式化、写裸磁盘……）照工具认出来的直接拒，人点了也不跑（`Hazard::refused`）；
//! 2. 别的高危命令（`git push --force` 这一类）问人，只能允许这一次或者拒绝：不提放行规则，高危的不该一劳永逸。认法照搬
//!    Miyu 的高危命令规则（工具自己的 `Tool::hazard`）；Miyu 的完全放开对这一类直接放行，这里照它工作区那一级问；
//! 3. 工具报的路径落在 LiteMiyu 自己的数据目录（`crate::store::paths` 的四处）里的拒，记忆的文件夹、人放技能的文件夹除外
//!    （能读能写：她要能给自己写技能，L5 五）；自带技能写出来的那一处只许读（Miyu 是开着的技能文件夹单独放开只读，`Shelf`）。
//!
//! Miyu 的权限级别、只读、沙盒、场所、主人放行的工具、只碰得到自己工作区的会话、知识库那几条都没有（L3 二删）。跑命令的
//! 工具不报路径，命令里碰数据目录的拦不住：Miyu 靠沙盒拦，这里没有沙盒。只有 `shell` 的高危认法照字面认出来的「删数据目录」
//! 拦得住（灭顶，直接拒）；数据目录交给认法的是 `paths.data`，配置、缓存、状态三处删了不算灭顶，照普通命令跑。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::fs::{ResolveError, real_path, resolve, resolve_itself};
use crate::store::paths::Paths;
use crate::tool::{Call, Catalog, Own, Stop, Target};
use lm_kernel::kernel::id::ModuleId;
use lm_kernel::kernel::raw::RawJson;
use lm_kernel::kernel::session::Verdict;
use lm_kernel::kernel::time::UtcOffset;
use lm_kernel::kernel::tool::{Access, Worded};

use crate::session::lettering::Lettering;

/// 一个会话的权限策略：工具目录、数据目录、家目录，和替它写拒绝那几句的字。
pub(crate) struct Guard {
    catalog: Arc<Catalog>,
    /// 不让工具碰的几处：配置、数据、缓存、状态四个目录。
    own: Vec<PathBuf>,
    /// 让碰的：记忆的文件夹（设计第五节第 7 条）、人放技能的文件夹（第 10 条，L5 五）。
    open: Vec<PathBuf>,
    /// 只许读的：自带技能写出来的那一处（`crate::skills::shipped`）。
    shelves: Vec<PathBuf>,
    /// 数据目录（`paths.data`）：交给工具，`write`、`edit` 照它认记忆的文件夹。
    data: PathBuf,
    /// 自己的几处和里面让碰的（LM-37）：交给工具认高危，删掉、挪走整个一处的命令是灭顶的（`shell` 的 `delete_data_root`）。
    places: Arc<Own>,
    home: Option<PathBuf>,
    lettering: Arc<Lettering>,
}

impl Guard {
    /// 照这一份工具目录、数据放的几处 `paths`、家目录 `home` 造。几处照真实位置比（链接过去的照样拦）。
    pub(crate) fn new(
        catalog: Arc<Catalog>,
        paths: &Paths,
        home: Option<PathBuf>,
        lettering: Arc<Lettering>,
    ) -> Guard {
        // 换成真实的位置，和判的时候换目标的是同一套（`fs::real_path`）：目录还没建的照最近的上级算。原来只
        // `canonicalize`，还没建的目录记的是原样的路径，上级里有链接时（macOS 的临时目录在 `/var` 下，`/var` 是到
        // `/private/var` 的链接）目标换出来的真实位置落不进去，自己的数据目录拦不住（LM-7，macOS 的 CI 上 guard 的三条测试红）。
        let real = |path: &Path| real_path(path).unwrap_or_else(|_| path.to_path_buf());
        // 认高危照命令的字面比：原样的、换成真实位置的都给（`~/.cache` 被链到别处时两种写法都认）。
        let both = |dirs: &[&PathBuf]| {
            let mut all: Vec<PathBuf> = Vec::new();
            for dir in dirs {
                for path in [dir.to_path_buf(), real(dir)] {
                    if !all.contains(&path) {
                        all.push(path);
                    }
                }
            }
            all
        };
        let places = Arc::new(Own {
            dirs: both(&[&paths.config, &paths.data, &paths.cache, &paths.state]),
            open: both(&[&paths.memory(), &paths.skills()]),
        });
        Guard {
            catalog,
            own: [&paths.config, &paths.data, &paths.cache, &paths.state]
                .into_iter()
                .map(|dir| real(dir))
                .collect(),
            open: vec![real(&paths.memory()), real(&paths.skills())],
            shelves: vec![real(&paths.shipped_skills())],
            data: paths.data.clone(),
            places,
            home,
            lettering,
        }
    }

    /// 判一次调用：工具名 `name`、修正过的参数 `args`、这一轮的工作目录 `cwd`。交回结论，和判的时候换成的真实位置（派的
    /// 时候交给工具）。这一轮加进来的目录、本会话放行过的规则不看：没有级别，没有要照它们放行的。
    pub(crate) fn judge(&self, name: &str, args: String, cwd: String) -> (Verdict, Vec<PathBuf>) {
        let mut checked = Vec::new();
        let verdict = self.verdict(name, args, cwd, &mut checked);
        (verdict, checked)
    }

    fn verdict(
        &self,
        name: &str,
        args: String,
        cwd: String,
        checked: &mut Vec<PathBuf>,
    ) -> Verdict {
        let Some(tool) = self.catalog.get(name) else {
            return Verdict::Allow;
        };
        let call = Call {
            args,
            cwd,
            home: self.home.clone(),
            data_root: Some(self.data.clone()),
            own: Some(Arc::clone(&self.places)),
            seen: Arc::default(),
            stop: Stop::default(),
            log: None,
            offset: UtcOffset::UTC,
            agents: None,
            messages: None,
            jobs: None,
            sessions: None,
            usage: None,
            questions: None,
            schedules: None,
            settings: None,
            checked: None,
            caller: None,
        };
        let cwd = PathBuf::from(&call.cwd);
        for target in tool.targets(&call) {
            let real = match self.real(&cwd, &target) {
                Ok(real) => real,
                Err(error) => {
                    let texts = self.lettering.guard();
                    return deny(texts.unresolvable(&target.path, &error.to_string()));
                }
            };
            if self.forbidden(&real, target.write) {
                return deny(self.lettering.guard().forbidden(&target.path));
            }
            checked.push(real);
        }
        match tool.hazard(&call) {
            Some(hazard) => hazardous(
                name,
                tool.spec().access.clone(),
                hazard,
                &tool.asking(&call),
            ),
            None => Verdict::Allow,
        }
    }

    /// 落在自己的数据目录里，又不在让碰的文件夹里（要写的，也不在只许读的那几处）。
    fn forbidden(&self, real: &Path, write: bool) -> bool {
        let inside = |dirs: &[PathBuf]| dirs.iter().any(|dir| real.starts_with(dir));
        !inside(&self.open) && (write || !inside(&self.shelves)) && inside(&self.own)
    }

    fn real(&self, cwd: &Path, target: &Target) -> Result<PathBuf, ResolveError> {
        let home = self.home.as_deref();
        if target.itself
            && let Some(real) = resolve_itself(cwd, home, &target.path)?
        {
            return Ok(real);
        }
        resolve(cwd, home, &target.path)
    }
}

/// 认出了高危的：灭顶的拒，写给模型工具交的那一句；别的问人，不提规则，说明是工具交的几格再加工具名和 `hazard`。
fn hazardous(
    name: &str,
    access: Access,
    hazard: crate::tool::Hazard,
    asking: &[(&'static str, String)],
) -> Verdict {
    if let Some(worded) = hazard.refused {
        return deny(worded);
    }
    let mut detail = json!({});
    for (key, value) in asking {
        detail[*key] = json!(value);
    }
    detail["tool"] = json!(name);
    detail["hazard"] = json!(hazard.code);
    Verdict::Ask {
        module: module(),
        access,
        rule: None,
        detail: Some(raw(&detail)),
    }
}

fn deny(worded: Worded) -> Verdict {
    Verdict::Deny {
        module: module(),
        text: worded.text,
        human: worded.said,
    }
}

fn module() -> ModuleId {
    ModuleId::parse("permissions").expect("permissions 合模块编号的写法")
}

fn raw(value: &Value) -> RawJson {
    lm_kernel::kernel::json::from_str(&value.to_string()).expect("自己拼的 JSON 读得回来")
}

#[cfg(test)]
mod tests;
