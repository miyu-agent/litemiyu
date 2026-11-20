//! 出厂的随核心附带的字（L3 二从测试夹具挪出来，引擎造会话要用）：编译时拿进程序，不读文件，读不了的错整个没有。
//! 提示词是 Miyu 出厂的软件工程师那一段（`docs/00-设计.md` 第五节第 15 条：没有 `prompt.md` 时用它）。

use crate::policy::drivers::DriverPlaceholders;
use crate::policy::facts::FactTexts;
use crate::policy::snapshot::{
    CompactionTexts, CoreTexts, PermissionTexts, ToolResultTexts, TurnEndedTexts,
};

/// 出厂的提示词：没有 `~/.config/litemiyu/prompt.md` 时用它。
pub const PROMPT: &str = include_str!("../../../../resources/core/prompt.md");

/// 出厂的随核心附带的字。
pub fn core() -> CoreTexts {
    CoreTexts {
        checkpoint_open: include_str!("../../../../resources/core/checkpoint-open.txt").to_string(),
        checkpoint_close: include_str!("../../../../resources/core/checkpoint-close.txt")
            .to_string(),
        checkpoint_end: include_str!("../../../../resources/core/checkpoint-end.txt").to_string(),
        turn_ended: TurnEndedTexts {
            interrupted: include_str!("../../../../resources/core/turn-ended/interrupted.txt")
                .to_string(),
            error: include_str!("../../../../resources/core/turn-ended/error.txt").to_string(),
            step_limit: include_str!("../../../../resources/core/turn-ended/step_limit.txt")
                .to_string(),
            aborted: include_str!("../../../../resources/core/turn-ended/aborted.txt").to_string(),
            restarted: include_str!("../../../../resources/core/turn-ended/restarted.txt")
                .to_string(),
        },
        facts: FactTexts {
            env: include_str!("../../../../resources/core/facts/env.txt").to_string(),
            reply_cut: include_str!("../../../../resources/core/facts/reply-cut.txt").to_string(),
            session: Some(include_str!("../../../../resources/core/facts/session.txt").to_string()),
            output_limit: Some(
                include_str!("../../../../resources/core/facts/output-limit.txt").to_string(),
            ),
            model: Some(include_str!("../../../../resources/core/facts/model.txt").to_string()),
        },
        tool_results: ToolResultTexts {
            unknown: include_str!("../../../../resources/core/tool-results/unknown.txt")
                .to_string(),
            not_an_object: include_str!(
                "../../../../resources/core/tool-results/not-an-object.txt"
            )
            .to_string(),
            cancelled_before: include_str!(
                "../../../../resources/core/tool-results/cancelled-before.txt"
            )
            .to_string(),
            cancelled_running: include_str!(
                "../../../../resources/core/tool-results/cancelled-running.txt"
            )
            .to_string(),
            skipped: include_str!("../../../../resources/core/tool-results/skipped.txt")
                .to_string(),
            denied: include_str!("../../../../resources/core/tool-results/denied.txt").to_string(),
            denied_with_reason: include_str!(
                "../../../../resources/core/tool-results/denied-with-reason.txt"
            )
            .to_string(),
            unattended: include_str!("../../../../resources/core/tool-results/unattended.txt")
                .to_string(),
            question_interrupted: include_str!(
                "../../../../resources/core/tool-results/question-interrupted.txt"
            )
            .to_string(),
            question_voided: include_str!(
                "../../../../resources/core/tool-results/question-voided.txt"
            )
            .to_string(),
            question_unattended: include_str!(
                "../../../../resources/core/tool-results/question-unattended.txt"
            )
            .to_string(),
            unavailable: include_str!("../../../../resources/core/tool-results/unavailable.txt")
                .to_string(),
            crashed: include_str!("../../../../resources/core/tool-results/crashed.txt")
                .to_string(),
            uninstalled: include_str!("../../../../resources/core/tool-results/uninstalled.txt")
                .to_string(),
            restarted: include_str!("../../../../resources/core/tool-results/restarted.txt")
                .to_string(),
        },
        permissions: PermissionTexts {
            forbidden: include_str!("../../../../resources/core/permissions/forbidden.txt")
                .to_string(),
            unresolvable: include_str!("../../../../resources/core/permissions/unresolvable.txt")
                .to_string(),
        },
        drivers: DriverPlaceholders {
            image_omitted: include_str!("../../../../resources/core/drivers/image-omitted.txt")
                .to_string(),
            file_omitted: include_str!("../../../../resources/core/drivers/file-omitted.txt")
                .to_string(),
            no_output: include_str!("../../../../resources/core/drivers/no-output.txt").to_string(),
            tool_attachments: include_str!(
                "../../../../resources/core/drivers/tool-attachments.txt"
            )
            .to_string(),
            tool_attachments_only: include_str!(
                "../../../../resources/core/drivers/tool-attachments-only.txt"
            )
            .to_string(),
            text_file: Some(crate::policy::TextFileTexts {
                file_open: include_str!("../../../../resources/core/drivers/file-open.txt")
                    .to_string(),
                file_cut: include_str!("../../../../resources/core/drivers/file-cut.txt")
                    .to_string(),
                file_close: include_str!("../../../../resources/core/drivers/file-close.txt")
                    .to_string(),
            }),
            image_name: Some(crate::policy::ImageNameTexts {
                image_open: include_str!("../../../../resources/core/drivers/image-open.txt")
                    .to_string(),
                image_close: include_str!("../../../../resources/core/drivers/image-close.txt")
                    .to_string(),
                image_omitted_named: include_str!(
                    "../../../../resources/core/drivers/image-omitted-named.txt"
                )
                .to_string(),
            }),
            image_description: Some(crate::policy::ImageDescriptionTexts {
                image_description_open: include_str!(
                    "../../../../resources/core/drivers/image-description-open.txt"
                )
                .to_string(),
                image_description_open_named: include_str!(
                    "../../../../resources/core/drivers/image-description-open-named.txt"
                )
                .to_string(),
                image_description_close: include_str!(
                    "../../../../resources/core/drivers/image-description-close.txt"
                )
                .to_string(),
            }),
            attached_path: Some(crate::policy::AttachedPathTexts {
                image_omitted_path: include_str!(
                    "../../../../resources/core/drivers/image-omitted-path.txt"
                )
                .to_string(),
                file_omitted_path: include_str!(
                    "../../../../resources/core/drivers/file-omitted-path.txt"
                )
                .to_string(),
            }),
            open_path: Some(crate::policy::OpenPathTexts {
                image_open_path: include_str!(
                    "../../../../resources/core/drivers/image-open-path.txt"
                )
                .to_string(),
                file_open_path: include_str!(
                    "../../../../resources/core/drivers/file-open-path.txt"
                )
                .to_string(),
            }),
        },
        compaction: Some(CompactionTexts {
            summarize_task: include_str!(
                "../../../../resources/core/compaction/summarize-task.txt"
            )
            .to_string(),
            summarize_instructions: include_str!(
                "../../../../resources/core/compaction/summarize-instructions.txt"
            )
            .to_string(),
            summarize_end: include_str!("../../../../resources/core/compaction/summarize-end.txt")
                .to_string(),
            rebuild: Some(crate::policy::RebuildTexts {
                notes_files: include_str!("../../../../resources/core/compaction/notes-files.txt")
                    .to_string(),
                notes_files_more: include_str!(
                    "../../../../resources/core/compaction/notes-files-more.txt"
                )
                .to_string(),
                notes_retrieve: include_str!(
                    "../../../../resources/core/compaction/notes-retrieve.txt"
                )
                .to_string(),
                notes_too_large: include_str!(
                    "../../../../resources/core/compaction/notes-too-large.txt"
                )
                .to_string(),
                restored_open: include_str!(
                    "../../../../resources/core/compaction/restored-open.txt"
                )
                .to_string(),
                restored_close: include_str!(
                    "../../../../resources/core/compaction/restored-close.txt"
                )
                .to_string(),
            }),
            summarize_system: Some(
                include_str!("../../../../resources/core/compaction/summarize-system.txt")
                    .to_string(),
            ),
            shorten: Some(crate::policy::ShortenTexts {
                truncated: include_str!("../../../../resources/core/compaction/truncated.txt")
                    .to_string(),
                notes_uncovered: include_str!(
                    "../../../../resources/core/compaction/notes-uncovered.txt"
                )
                .to_string(),
            }),
        }),
        jobs: Some(jobs()),
        // 到点交进来的话的标签（L5 四，循环）：固定间隔的循环用 `schedule` 那一对，她自己定节奏的用 `loop` 那一对。
        schedule: Some(crate::policy::ScheduleTexts {
            schedule_open: include_str!(
                "../../../../resources/software/schedule/common/message-open.txt"
            )
            .to_string(),
            schedule_close: include_str!(
                "../../../../resources/software/schedule/common/message-close.txt"
            )
            .to_string(),
            loop_open: include_str!("../../../../resources/software/loop/common/message-open.txt")
                .to_string(),
            loop_close: include_str!(
                "../../../../resources/software/loop/common/message-close.txt"
            )
            .to_string(),
        }),
        peers: Some(crate::policy::PeerTexts {
            message_open: include_str!("../../../../resources/core/peers/message-open.txt")
                .to_string(),
            message_close: include_str!("../../../../resources/core/peers/message-close.txt")
                .to_string(),
            idle: Some(crate::policy::PeerIdleTexts {
                idle_open: include_str!("../../../../resources/core/peers/idle-open.txt")
                    .to_string(),
                idle_silent: include_str!("../../../../resources/core/peers/idle-silent.txt")
                    .to_string(),
                idle_expired: include_str!("../../../../resources/core/peers/idle-expired.txt")
                    .to_string(),
                idle_gone: include_str!("../../../../resources/core/peers/idle-gone.txt")
                    .to_string(),
                idle_close: include_str!("../../../../resources/core/peers/idle-close.txt")
                    .to_string(),
            }),
        }),
        recap: Some(crate::policy::RecapTexts {
            user: include_str!("../../../../resources/core/recap/user.txt").to_string(),
            assistant: include_str!("../../../../resources/core/recap/assistant.txt").to_string(),
            omitted: include_str!("../../../../resources/core/recap/omitted.txt").to_string(),
            excerpted: include_str!("../../../../resources/core/recap/excerpted.txt").to_string(),
        }),
        title: Some(crate::policy::TitleTexts {
            instruction: include_str!("../../../../resources/core/title/instruction.txt")
                .to_string(),
        }),
        vision: Some(crate::policy::VisionTexts {
            instruction: include_str!("../../../../resources/core/vision/instruction.txt")
                .to_string(),
            question: include_str!("../../../../resources/core/vision/question.txt").to_string(),
        }),
    }
}

/// 出厂的两种回报的写法（施工 7-2）。
fn jobs() -> crate::policy::JobTexts {
    crate::policy::JobTexts {
        command_open: include_str!("../../../../resources/core/jobs/command-open.txt").to_string(),
        command_exit: include_str!("../../../../resources/core/jobs/command-exit.txt").to_string(),
        command_signal: include_str!("../../../../resources/core/jobs/command-signal.txt")
            .to_string(),
        command_duration: include_str!("../../../../resources/core/jobs/command-duration.txt")
            .to_string(),
        command_output: include_str!("../../../../resources/core/jobs/command-output.txt")
            .to_string(),
        command_close: include_str!("../../../../resources/core/jobs/command-close.txt")
            .to_string(),
        subagent_open: include_str!("../../../../resources/core/jobs/subagent-open.txt")
            .to_string(),
        subagent_person: include_str!("../../../../resources/core/jobs/subagent-person.txt")
            .to_string(),
        subagent_truncated: include_str!("../../../../resources/core/jobs/subagent-truncated.txt")
            .to_string(),
        subagent_silent: include_str!("../../../../resources/core/jobs/subagent-silent.txt")
            .to_string(),
        subagent_close: include_str!("../../../../resources/core/jobs/subagent-close.txt")
            .to_string(),
        subagent_omitted: include_str!("../../../../resources/core/jobs/subagent-omitted.txt")
            .to_string(),
        stopped_by_user: include_str!("../../../../resources/core/jobs/stopped-by-user.txt")
            .to_string(),
        subagent_message_open: include_str!(
            "../../../../resources/core/jobs/subagent-message-open.txt"
        )
        .to_string(),
        subagent_message_close: include_str!(
            "../../../../resources/core/jobs/subagent-message-close.txt"
        )
        .to_string(),
        command_tail: include_str!("../../../../resources/core/jobs/command-tail.txt").to_string(),
        command_tail_cut: include_str!("../../../../resources/core/jobs/command-tail-cut.txt")
            .to_string(),
        user_message_open: include_str!("../../../../resources/core/jobs/user-message-open.txt")
            .to_string(),
        user_message_close: include_str!("../../../../resources/core/jobs/user-message-close.txt")
            .to_string(),
        lines_open: include_str!("../../../../resources/core/jobs/lines-open.txt").to_string(),
        lines_close: include_str!("../../../../resources/core/jobs/lines-close.txt").to_string(),
        shell_open: include_str!("../../../../resources/core/jobs/shell-open.txt").to_string(),
        shell_command: include_str!("../../../../resources/core/jobs/shell-command.txt")
            .to_string(),
        shell_close: include_str!("../../../../resources/core/jobs/shell-close.txt").to_string(),
    }
}
