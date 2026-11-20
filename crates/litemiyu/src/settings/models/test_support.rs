//! 「供应商和模型」测试用的样例（照 Miyu 终端 `settings/test_support.rs`）：两家供应商（一家中转站带组织前缀）、默认用途。

use lm_kernel::models::facts::{Fact, Facts, Source, Wire};

use super::data::{Data, Model, Provider};

/// 一个模型的资料：窗口、能收的输入，别的照驱动的保守默认。
pub fn facts(window: u64, inputs: &[&str]) -> Facts {
    let fact = |value| Fact {
        value,
        source: Source::Default,
    };
    Facts {
        window: fact(Some(window)),
        max_output: Fact {
            value: None,
            source: Source::Default,
        },
        inputs: Fact {
            value: inputs.iter().map(|i| (*i).to_string()).collect(),
            source: Source::Default,
        },
        tools: Fact {
            value: None,
            source: Source::Default,
        },
        reasoning: Fact {
            value: None,
            source: Source::Default,
        },
        effort: Fact {
            value: None,
            source: Source::Default,
        },
        takes_temperature: Fact {
            value: None,
            source: Source::Default,
        },
        temperature: Fact {
            value: None,
            source: Source::Default,
        },
        name: Fact {
            value: String::new(),
            source: Source::Default,
        },
        status: Fact {
            value: None,
            source: Source::Default,
        },
        wire: Wire::default(),
    }
}

fn model(provider: &str, name: &str, window: u64, inputs: &[&str]) -> Model {
    Model {
        name: name.to_string(),
        reference: format!("{provider}/{name}"),
        listed: "config",
        facts: Some(facts(window, inputs)),
    }
}

/// 读好的一份。
pub fn sample() -> Data {
    Data {
        providers: vec![
            Provider {
                id: "dev".into(),
                driver: Some("openai-chat".into()),
                base_url: Some("env:LITEMIYU_DEV_BASE_URL".into()),
                models: vec![model("dev", "flash", 128_000, &["text"])],
                ..Provider::default()
            },
            Provider {
                id: "relay".into(),
                name: Some("中转站".into()),
                base_url: Some("https://relay.example.invalid/v1".into()),
                models: vec![
                    model("relay", "cline/deepseek-v4", 1_000_000, &["text"]),
                    model("relay", "zhipu/glm-5v", 64_000, &["text", "image"]),
                    model("relay", "gpt-oss", 128_000, &["text"]),
                ],
                ..Provider::default()
            },
        ],
        chat: Some("dev/flash".into()),
        vision: Some("relay/zhipu/glm-5v".into()),
    }
}
