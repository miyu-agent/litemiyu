//! 技能开头的读法和 yaml-rust2 对照（施工 LM-1）：同一批样本，yaml-rust2 读得出的，这里读出来一模一样；yaml-rust2 读不出的，
//! 这里也读不出。认不得的写法报读不了、说出第几行。

use yaml_rust2::{Yaml, YamlLoader};

use super::{Unreadable, Value, parse};

/// yaml-rust2 读出来的，换成同样的值；读不出的交回它的原话。
fn reference(front: &str) -> Result<Value, String> {
    let docs = YamlLoader::load_from_str(front).map_err(|error| error.to_string())?;
    Ok(docs.first().map_or(Value::Null, convert))
}

fn convert(yaml: &Yaml) -> Value {
    match yaml {
        Yaml::String(text) => Value::Text(text.clone()),
        Yaml::Boolean(on) => Value::Bool(*on),
        Yaml::Integer(_) | Yaml::Real(_) => Value::Number,
        Yaml::Null => Value::Null,
        Yaml::Array(items) => Value::List(items.iter().map(convert).collect()),
        Yaml::Hash(hash) => {
            Value::Map(hash.iter().map(|(k, v)| (convert(k), convert(v))).collect())
        }
        other => panic!("样本里不该有 {other:?}"),
    }
}

/// 现有技能测试里出现过的、出厂技能的、Claude 和 Miyu 技能常见的开头写法。
const SAMPLES: &[&str] = &[
    "",
    "# 只有注释\n\n",
    "name: a\ndescription: Hi.\n",
    "name: pdf-processing\ndescription: Extract PDF text, fill forms. Use when handling PDFs.\nlicense: Apache-2.0\nmetadata:\n  author: example-org\n  version: \"1.0\"\n",
    "name: a\ndescription: \"Say: hi, \\\"you\\\"\"\n",
    "name: a\ndescription: >\n  First line\n  goes on.\n",
    "name: a\ndescription: |\n  First line.\n  Second\tline.\n",
    "name: a\r\ndescription: Hi.\r\n",
    "description: Hi.\n",
    "name: [a]\ndescription: Hi.\n",
    "name: a\ndescription: \"  \"\n",
    "name: a\ndescription: [unclosed\n",
    "name: a\ndescription: A.\nmetadata:\n  display_name: Write a skill\n  display_name_zh: 写技能\n  display_name_ja: \"\"\n  summary_zh: 照包的样子写\n",
    "name: a\ndescription: Use this skill when: the user says \"PDF\" or a\\b\nlicense: MIT\n",
    "name: a\ndescription: \"Say: hi\"\nlicense: MIT: or Apache\n",
    "name: a\ndescription: Hi.\ncompatibility: Needs git\nmetadata:\n  version: 1.0\n",
    "name: a\ndescription: Hi.\ncompatibility: 3\n",
    "name: a\ndescription: A.\ndisable-model-invocation: true\n",
    "name: a\ndescription: A.\nuser-invocable: false\n",
    "name: a\ndescription: A.\ndisable-model-invocation: \"yes\"\nuser-invocable: 0\n",
    // 出厂的 skill-creator。
    "name: skill-creator\ndescription: \"Write a new LiteMiyu skill or change one. Use when asked to make, write or improve a skill (写技能、做个技能).\"\nmetadata:\n  display_name: Write a skill\n  display_name_zh: 写技能\n  summary: Write a skill into the skills folder\n  summary_zh: 在技能文件夹里写一个技能\n",
    // 注释、空值、各种是非和数、空。
    "name: a # 行末注释\ndescription: a#b c # 注释\n# 整行注释\nlicense:\nx: ~\ny: null\nz: True\nw: FALSE\nv: yes\nu: 0x1F\nt: -3\ns: 1e5\nr: .inf\nq: inf\n",
    "name: 'it''s'\ndescription: 'Say: \"hi\"'\n",
    "name: a\ndescription: \"tab\\there\\nline \\u00e9 \\x41 \\\\ \\/\"\n",
    // 列表：一行的、缩进的、和键对齐的、加引号的。
    "name: a\ndescription: A.\nallowed-tools: [Read, Write, \"Bash(git:*)\"]\n",
    "name: a\ndescription: A.\nallowed-tools:\n  - Bash\n  - Read\nlicense: MIT\n",
    "name: a\ndescription: A.\nallowed-tools:\n- Bash\n- 'Read'\n",
    "name: a\ndescription: A.\nallowed-tools: []\n",
    // 折行的几种。
    "name: a\ndescription: |-\n  one\n\n  two\n",
    "name: a\ndescription: >-\n  one\n  two\n\n  three\n",
    "name: a\ndescription: |+\n  one\n\n\nlicense: MIT\n",
    "name: a\ndescription: >\n  one\n    indented\n  two\n\n",
    "name: a\ndescription: |\n    four spaces\n      six\n    four\n",
    "name: a\ndescription: >\n\n  after a blank\n",
    "name: a\ndescription: |\nlicense: MIT\n",
    // 不加引号、往下接着写的一句。
    "name: a\ndescription: Long text that\n  goes on here\n  and here.\nlicense: MIT\n",
    "name: a\ndescription: First\n\n  second paragraph\n",
    // 别的写法。
    "name: a\ndescription: 中文说明，带全角：冒号\nmetadata:\n  a: 1\n  nested:\n    deep: x\n",
    "name: a\ndescription: x\nmetadata:\n    four: spaces\n",
    "name: a\ndescription: \"x\" # 注释\n",
    "name: a\ndescription: url http://example.com/a#frag\n",
    "name: a\ndescription: ends with colon:\n",
    "name: a\nname: b\ndescription: dup\n",
];

#[test]
fn the_samples_read_the_same_as_yaml_rust2() {
    for front in SAMPLES {
        let ours = parse(front);
        match reference(front) {
            Ok(theirs) => assert_eq!(ours, Ok(theirs), "{front:?}"),
            Err(why) => assert!(
                ours.is_err(),
                "yaml-rust2 读不出（{why}）、这里读出了 {ours:?}：{front:?}"
            ),
        }
    }
}

/// 认不得的写法报读不了、说出 `SKILL.md` 的第几行（第 1 行是开头的 `---`），不猜着读。
#[test]
fn what_it_does_not_understand_is_unreadable_at_a_line() {
    let cases = [
        ("name: a\ndescription: &anchor Hi.\n", 3),
        ("name: a\ndescription: *alias\n", 3),
        ("name: a\ndescription: !!str Hi.\n", 3),
        ("name: a\nmetadata: {a: b}\n", 3),
        ("name: a\ndescription: \"open\n  close\"\n", 3),
        ("name: a\n? complex\n", 3),
        ("name: a\n\tdescription: x\n", 3),
        ("name: a\nallowed-tools:\n  - name: x\n", 4),
        ("name: a\ndescription: |2\n   x\n", 3),
        ("name: a\n\"quoted\": x\n", 3),
        ("name: a\ndescription: x\n  - y: z\n", 4),
        ("  name: a\n", 2),
    ];
    for (front, line) in cases {
        match parse(front) {
            Err(Unreadable { line: at, .. }) => assert_eq!(at, line, "{front:?}"),
            Ok(value) => panic!("该读不了：{front:?} 读成了 {value:?}"),
        }
    }
}
