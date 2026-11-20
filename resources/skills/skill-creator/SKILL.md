---
name: skill-creator
description: "Write a new LiteMiyu skill or change one. Use when asked to make, write or improve a skill (写技能、做个技能)."
metadata:
  display_name: Write a skill
  display_name_zh: 写技能
  summary: Write a skill into the skills folder
  summary_zh: 在技能文件夹里写一个技能
---

# Write a skill

A skill is a folder with a `SKILL.md`: a short head that says when to use it, then a body that teaches one kind of task. You only see the name and description until you read the body, so the description decides when the skill gets used.

## Where it goes

Write the skill folder into `{skills}`:

```text
<skill-name>/
└── SKILL.md      # plus any reference files the body points to
```

The skill shows up from the next session. To change it later, edit the files there.

## The head

```yaml
---
name: take-notes
description: "Keep tidy notes in notes/. Use when asked to note, log or record something (记一下、做笔记)."
metadata:
  display_name: Take notes
  display_name_zh: 记笔记
  summary: Keeps notes in one place
  summary_zh: 把笔记记在一个地方
---
```

- `name`: 1 to 64 lowercase letters, digits and single `-`, the same as the folder.
- `description`: English, what it does and when to use it, up to 1024 characters. A few words people really say may be in Chinese. Put it in double quotes: a bare `: ` inside breaks the YAML and the skill is skipped.
- `metadata`: names and a one-line summary for people, `_zh` and `_ja` for other languages. Optional.

## The body

- One task per skill. Steps in order, short sentences, the commands or file formats to use.
- Put long references in separate files next to `SKILL.md` and say when to read them.
- Do not repeat what every task needs; write what is special to this one.
