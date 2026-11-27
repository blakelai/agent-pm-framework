use crate::{
    baselines,
    grounding::Packet,
    inspection,
    provider::Call,
    records::Records,
    workspace::{Workspace, hash, parse_note},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeSet, fmt::Write, fs};

const BASELINES: &str = "00_Governance/Baselines";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preview {
    pub id: String,
    pub path: String,
    pub text: String,
    pub(crate) packet_sha256: String,
    call_sha256: String,
    baselines_sha256: String,
    pub(crate) confirmed_at: Option<String>,
}

impl Preview {
    pub(crate) fn new(w: &Workspace, call: &Call, args: &Value, packet: &Packet) -> Result<Self> {
        let before = if let Some(expected) = args["expected_hash"].as_str() {
            let before = w.read(&packet.path)?;
            ensure!(
                hash(before.as_bytes()) == expected,
                "Original changed before preview"
            );
            before
        } else {
            String::new()
        };
        let diff = inspection::diff(&before, &packet.candidate)?;
        let records = Records::load(w)?;
        let baselines = baseline_versions(&records, packet)?;
        let mut text = format!("{}\n\nProposed changes\n", packet.path);
        for line in diff.lines.iter().filter(|line| line.kind != ' ') {
            let number = line.old.or(line.new).unwrap_or_default();
            writeln!(text, "{} {number:>4}  {}", line.kind, line.text)?;
        }
        if diff.old_sha256 == diff.new_sha256 {
            text.push_str("No changes.\n");
        } else if diff.old_final_newline != diff.new_final_newline {
            writeln!(
                text,
                "Final newline: {}",
                if diff.new_final_newline {
                    "added"
                } else {
                    "removed"
                }
            )?;
        }
        text.push_str("\nCited evidence (including traced sources)\n");
        for source in &packet.sources {
            writeln!(text, "- {} · SHA256 {}", source.path, source.sha256)?;
        }
        if !baselines.is_empty() {
            text.push_str("\nApplicable baseline manifests\n");
            for baseline in &baselines {
                writeln!(text, "- {} · SHA256 {}", baseline.id, baseline.manifest)?;
            }
        }
        text.push_str("\nOpen questions\n");
        let mut open = records
            .active
            .iter()
            .filter(|d| d.kind == "question" && d.metadata["workflow_status"] == "open")
            .peekable();
        if open.peek().is_none() {
            text.push_str("None recorded.\n");
        }
        for question in open {
            writeln!(text, "- {} · {}", question.path, question.title)?;
        }
        ensure!(
            text.len() <= 96 * 1024,
            "Draft preview exceeds 96 KiB; narrow the proposal so it can be reviewed in full"
        );
        Ok(Self {
            id: uuid::Uuid::new_v4().to_string(),
            path: packet.path.clone(),
            text,
            packet_sha256: packet.fingerprint()?,
            call_sha256: hash(&serde_json::to_vec(call)?),
            baselines_sha256: hash(&serde_json::to_vec(&baselines)?),
            confirmed_at: None,
        })
    }

    pub(crate) fn ensure_current(&self, w: &Workspace, call: &Call, args: &Value) -> Result<()> {
        ensure!(
            self.call_sha256 == hash(&serde_json::to_vec(call)?),
            "Draft call changed"
        );
        let packet = Packet::prepare(w, args)?;
        ensure!(
            self.packet_sha256 == packet.fingerprint()?,
            "Draft or evidence changed"
        );
        let baselines = baseline_versions(&Records::load(w)?, &packet)?;
        ensure!(
            self.baselines_sha256 == hash(&serde_json::to_vec(&baselines)?),
            "Applicable baseline versions or decisions changed"
        );
        Ok(())
    }
}

#[derive(Serialize)]
struct BaselineVersion {
    id: String,
    manifest: String,
    decisions: Vec<String>,
}

fn baseline_versions(records: &Records, packet: &Packet) -> Result<Vec<BaselineVersion>> {
    let (candidate, _) = parse_note(&packet.candidate)?;
    let id = candidate["id"].as_str().context("Draft needs an ID")?;
    let w = &records.workspace;
    let mut applicable = BTreeSet::new();
    for source in &packet.sources {
        if let Some((id, _)) = source
            .path
            .strip_prefix(&format!("{BASELINES}/"))
            .and_then(|p| p.split_once('/'))
        {
            applicable.insert(id.to_owned());
        }
        if let Some(document) = records.active.iter().find(|d| d.path == source.path)
            && let Some(id) = document.metadata["baseline_id"].as_str()
        {
            applicable.insert(id.to_owned());
        }
    }
    let directory = w.path(BASELINES)?;
    if directory.exists() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("Baseline directory name must be UTF-8"))?;
            let path = format!("{BASELINES}/{name}/manifest.json");
            if w.path(&path)?.is_file() {
                let manifest: Value = serde_json::from_str(&w.read(&path)?)
                    .with_context(|| format!("Invalid baseline manifest: {path}"))?;
                if manifest["documents"].get(id).is_some() {
                    applicable.insert(name);
                }
            }
        }
    }
    applicable
        .into_iter()
        .map(|id| {
            let (_, sha256) = baselines::load(w, &id)?;
            let mut decisions = records
                .active
                .iter()
                .filter(|d| {
                    d.kind == "decision"
                        && d.metadata["decision_type"] == "baseline_approval"
                        && d.metadata["baseline_id"] == id
                })
                .map(|d| d.hash.clone())
                .collect::<Vec<_>>();
            decisions.sort();
            Ok(BaselineVersion {
                id,
                manifest: sha256,
                decisions,
            })
        })
        .collect()
}
