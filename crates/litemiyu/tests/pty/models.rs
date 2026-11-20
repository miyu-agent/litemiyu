//! 「供应商和模型」在伪终端里走一遍（配置线返工第二趟）：两个分页、几栏、编辑窗、模型资料窗、加一家的目录窗、选默认模型的窗
//! （LM-8 砍了模型池：原来的「模型池」分页、池的编辑窗、删固定池的提示没了），每一屏照 Miyu 终端的存屏（`settings-models-*.txt`）的名字存一份，配置不改。
//! 数据都在临时目录，供应商的地址是连不上的本机端口，key 是环境变量的引用（不设）。

use crate::support::{Options, Session};

const ESC: &[u8] = b"\x1b";

/// 和 Miyu 存屏时那一份差不多：一家中转站，两个带组织前缀的模型，默认文本模型指到其中一个。
const CONFIG: &str = "[providers.relay]\ndriver = \"openai-chat\"\nbase_url = \"http://127.0.0.1:9/v1\"\nkey = { env = \"RELAY_KEY\" }\n\n\
[providers.relay.models.\"cline/v4\"]\nwindow = 128000\n\n[providers.relay.models.\"zhipu/glm\"]\nwindow = 64000\ninputs = [\"text\", \"image\"]\n\n\
[models]\nchat = \"relay/cline/v4\"\n";

#[test]
fn the_models_page_walks_like_miyu_and_changes_nothing() {
    let options = Options {
        cols: Some(100),
        ..Options::default()
    };
    let session = Session::launch_with(Some(CONFIG), &[], &["config"], &options);
    session.wait_for("Enter 进入", "主菜单");
    session.wait_for("供应商、默认模型", "主菜单里那一项的第二行");
    assert!(!session.screen().contains("模型池"), "{}", session.screen());
    session.type_text("jj");
    session.press_enter();
    session.wait_for(" 供应商  ·  默认模型", "两个分页");
    assert!(!session.screen().contains("模型池"), "{}", session.screen());
    session.wait_for("┃ relay", "供应商栏选中 relay");
    session.wait_for("zhipu", "组织栏");
    session.shot("settings-models-providers");
    session.type_text("l");
    session.wait_for("┃ 全部", "焦点到组织栏");
    session.shot("settings-models-orgs");
    session.type_text("l");
    session.wait_for("┃ cline/v4", "焦点到模型栏");
    session.shot("settings-models-models");
    session.press_enter();
    session.wait_for("上下文窗口", "模型资料窗");
    session.wait_for("思考程度", "模型资料窗的几格");
    session.shot("settings-models-model-edit");
    session.press(ESC);
    session.wait_until("模型资料窗关上", |s| {
        !s.screen().contains("上下文窗口")
    });
    session.type_text("hh");
    session.wait_for("┃ relay", "焦点回供应商栏");
    session.press_enter();
    session.wait_for("编辑供应商 · relay", "供应商的编辑窗");
    session.wait_for("[ 保存 s ]  [ 取消 Esc ]", "两个按钮");
    session.shot("settings-models-provider-edit");
    session.press(ESC);
    session.wait_until("编辑窗关上", |s| !s.screen().contains("编辑供应商"));
    session.type_text("a");
    session.wait_for("更多供应商…", "加一家的目录窗");
    session.wait_for("/ 搜索", "筛的那一行");
    session.shot("settings-models-add");
    for _ in 0..60 {
        session.type_text("j");
    }
    session.press_enter();
    session.wait_for("小写字母开头，字母、数字、-、_", "自定义的编辑窗");
    session.shot("settings-models-add-custom");
    session.press(ESC);
    session.wait_until("编辑窗关上", |s| !s.screen().contains("小写字母开头"));
    session.type_text(".");
    session.wait_for("默认视觉模型", "默认模型分页");
    session.wait_for("当前  relay/cline/v4", "右边写现在用的");
    session.shot("settings-models-defaults");
    session.press_enter();
    session.wait_for("relay/zhipu/glm", "选模型的窗：照供应商分段列模型");
    assert!(
        !session.screen().contains("模型池"),
        "没有池那一段：{}",
        session.screen()
    );
    session.shot("settings-models-defaults-pick");
    session.press(ESC);
    session.wait_until("选模型的窗关上", |s| {
        !s.screen().contains("relay/zhipu/glm")
    });
    session.type_text(".");
    session.wait_for(
        "当前  relay/cline/v4",
        "默认模型是最后一页，再按 . 停在这里",
    );
    session.press(ESC);
    session.wait_for("Enter 进入", "回主菜单");
    session.press(ESC);
    assert!(session.wait_exit(), "主菜单 Esc 退出程序");
    let config = std::fs::read_to_string(
        lm_engine::store::paths::Paths::under(session.data()).config_file(),
    )
    .expect("配置在");
    assert_eq!(config, CONFIG, "走一遍什么都没改");
}
