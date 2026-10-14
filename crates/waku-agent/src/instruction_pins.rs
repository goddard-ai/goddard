//! The small, revision-aware CLI surface for temporary Boss obligations.
use super::*;
use waku_protocol::instruction_pins::{
    InstructionPinOperation as Action, InstructionPinRetirement,
};

pub(super) fn run(operation: &str, args: Vec<String>) -> anyhow::Result<()> {
    let action = parse(operation, args)?;
    print_boss(boss_request(
        waku_protocol::boss::BossOperation::InstructionPin { action },
    )?)
}

fn parse(operation: &str, args: Vec<String>) -> anyhow::Result<Action> {
    if operation == "show" {
        let (pos, _) = flags(args, &[], true)?;
        if pos.len() != 1 {
            bail!("usage: boss instructions show ID");
        }
        return Ok(Action::Show {
            id: pos[0].parse().context("invalid pin ID")?,
        });
    }
    if operation == "list" {
        let (pos, opts) = flags(args, &["all"], true)?;
        if !pos.is_empty() {
            bail!("usage: boss instructions list [--all]");
        }
        return Ok(Action::List {
            include_retired: opts.contains_key("all"),
        });
    }
    if operation == "retire" {
        let (pos, opts) = flags(args, &["revision", "evidence", "superseded-by"], true)?;
        if pos.len() != 1 {
            bail!(
                "usage: boss instructions retire ID --revision N --evidence TEXT [--superseded-by ID]"
            );
        }
        return Ok(Action::Retire {
            id: pos[0].parse().context("invalid pin ID")?,
            expected_revision: opts
                .get("revision")
                .ok_or_else(|| anyhow!("--revision is required"))?
                .parse()
                .context("invalid revision")?,
            evidence: opts
                .get("evidence")
                .cloned()
                .ok_or_else(|| anyhow!("--evidence is required"))?,
            superseded_by: opts
                .get("superseded-by")
                .map(|id| id.parse().context("invalid replacement ID"))
                .transpose()?,
        });
    }
    if !matches!(operation, "pin" | "edit") {
        bail!("run boss instructions --help for valid commands");
    }
    let allowed = [
        "text",
        "file",
        "retire-when",
        "project-id",
        "retire-outcome",
        "source-session",
        "source-message",
        "revision",
    ];
    let (pos, opts) = flags(args, &allowed, true)?;
    let instruction = raw_input(&opts)?;
    let retire_when = opts
        .get("retire-when")
        .cloned()
        .ok_or_else(|| anyhow!("--retire-when is required"))?;
    let project_id = opts
        .get("project-id")
        .map(|id| id.parse().context("invalid project ID"))
        .transpose()?;
    let retire_on = opts
        .get("retire-outcome")
        .map(|id| {
            Ok::<_, anyhow::Error>(InstructionPinRetirement::OutcomeCompleted {
                outcome_id: id.parse().context("invalid outcome ID")?,
            })
        })
        .transpose()?;
    if operation == "edit" {
        if pos.len() != 1
            || opts.contains_key("source-session")
            || opts.contains_key("source-message")
        {
            bail!(
                "usage: boss instructions edit ID --revision N --text|--file --retire-when CONDITION [--project-id ID] [--retire-outcome ID]"
            );
        }
        return Ok(Action::Edit {
            id: pos[0].parse().context("invalid pin ID")?,
            expected_revision: opts
                .get("revision")
                .ok_or_else(|| anyhow!("--revision is required"))?
                .parse()
                .context("invalid revision")?,
            instruction,
            retire_when,
            project_id,
            retire_on,
        });
    }
    if !pos.is_empty() || opts.contains_key("revision") {
        bail!(
            "usage: boss instructions pin --text|--file --retire-when CONDITION --source-session ID --source-message ID [--project-id ID] [--retire-outcome ID]"
        );
    }
    let source_session_id = opts
        .get("source-session")
        .map(|id| id.parse().context("invalid source session ID"))
        .transpose()?;
    let source_message_id = opts
        .get("source-message")
        .map(|id| id.parse().context("invalid source message ID"))
        .transpose()?;
    if source_session_id.is_some() != source_message_id.is_some() {
        bail!("source-session and source-message must be provided together");
    }
    Ok(Action::Pin {
        instruction,
        retire_when,
        project_id,
        retire_on,
        source_session_id,
        source_message_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retirement_requires_a_revision_and_evidence() {
        let id = Uuid::new_v4().to_string();
        assert!(
            parse(
                "retire",
                vec![id.clone(), "--evidence".into(), "Completed".into()]
            )
            .is_err()
        );
        assert!(parse("retire", vec![id.clone(), "--revision".into(), "3".into()]).is_err());
        assert!(matches!(
            parse(
                "retire",
                vec![
                    id,
                    "--revision".into(),
                    "3".into(),
                    "--evidence".into(),
                    "User released the hold".into()
                ]
            )
            .unwrap(),
            Action::Retire {
                expected_revision: 3,
                ..
            }
        ));
    }
}
