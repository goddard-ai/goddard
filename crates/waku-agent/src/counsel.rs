use super::*;
use waku_protocol::counsel::CounselOperation;

pub(super) fn run(mut args: Vec<String>) -> anyhow::Result<()> {
    let action = if args.is_empty() {
        "view".to_owned()
    } else {
        args.remove(0)
    };
    let operation = match action.as_str() {
        "view" => {
            let (pos, _) = flags(args, &[], true)?;
            anyhow::ensure!(pos.is_empty(), "usage: boss counsel view");
            CounselOperation::View
        }
        "propose" => {
            let (_, opts) = flags(args, &["json", "json-file"], false)?;
            let mut value: serde_json::Value = serde_json::from_str(&json_input(&opts)?)?;
            anyhow::ensure!(value.is_object(), "Counsel proposal must be a JSON object");
            value["type"] = json!("propose");
            serde_json::from_value(value).context("invalid Counsel proposal")?
        }
        "complete" => {
            let (pos, opts) = flags(args, &["revision", "evidence"], true)?;
            anyhow::ensure!(
                pos.len() == 1,
                "usage: boss counsel complete ID --revision N --evidence TEXT"
            );
            CounselOperation::Complete {
                id: pos[0].parse().context("invalid recommendation ID")?,
                expected_revision: opts
                    .get("revision")
                    .ok_or_else(|| anyhow!("--revision is required"))?
                    .parse()
                    .context("invalid revision")?,
                evidence: opts
                    .get("evidence")
                    .cloned()
                    .ok_or_else(|| anyhow!("--evidence is required"))?,
            }
        }
        _ => bail!("run boss counsel --help for valid commands"),
    };
    print_boss(boss_request(waku_protocol::boss::BossOperation::Counsel {
        action: operation,
    })?)
}
