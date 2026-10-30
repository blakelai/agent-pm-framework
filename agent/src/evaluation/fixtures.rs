use super::*;

pub fn cases() -> Vec<Case> {
    vec![
        Case { id: "missing-inputs".into(), title: "Missing approval rule and staffing".into(), stage: "workflow".into(), purpose: "Observe an actual unresolved question and no dependent requirements or schedule.".into() },
        Case { id: "draft-chain".into(), title: "Sourced glossary and requirement chain".into(), stage: "workflow".into(), purpose: "Observe linked glossary, BRD, PRD, requirement and PBI drafts with current citations.".into() },
        Case { id: "draft-stages".into(), title: "Requirement workflow by stage".into(), stage: "language".into(), purpose: "Evaluate glossary, BRD, PRD, requirement, PBI and verification in separate sessions using actual preceding outputs.".into() },
        Case { id: "wiki-code-conflict".into(), title: "Wiki and implementation disagree".into(), stage: "system".into(), purpose: "Observe inspection of both pinned sources and a question instead of an invented business rule.".into() },
        Case { id: "capacity-forecast".into(), title: "Capacity shortage with an unestimated item".into(), stage: "estimate".into(), purpose: "Observe the native forecast, explicit overload and preserved unknown estimates.".into() },
    ]
}

// Copy only framework guidance and standard Skills. Customer records and custom
// Skills, repository files, runtime settings and sessions are never seed inputs.
pub fn seed(source: &Workspace, dest: &Workspace) -> Result<()> {
    for path in [
        "AGENTS.md",
        "HOME.md",
        "References.md",
        "tools/README.md",
        "00_Governance/Language-Settings.md",
        "00_Governance/Project-Setup.md",
        "00_Governance/Workflow.md",
        "00_Governance/Schema.md",
        "00_Governance/Baselines-and-Changes.md",
        "00_Governance/Discovery-and-Prioritization.md",
        "02_Domain/Code-Mappings.md",
        "03_Systems/OpenWiki-Integration.md",
        "03_Systems/Integration-Review.md",
        "08_Verification/Acceptance-and-Release.md",
        "09_Operations/Benefits-and-Costs.md",
        "90_Agent/Working-with-Agents.md",
        "90_Agent/Evidence-and-Clarification-Policy.md",
        "90_Agent/OKF-Profile.md",
        "90_Agent/Language-Policy.md",
        "90_Agent/Skills-Index.md",
        "90_Agent/Evals/Evaluation-Method.md",
    ]
    .into_iter()
    .chain(crate::workspace::GUIDES.iter().copied())
    {
        put(dest, path, source.read(path)?.as_bytes())?;
    }
    put_note(
        dest,
        "01_Sources/Source-Index.md",
        json!({"type":"Reference","title":"Synthetic source index","language":"en"}),
        "# Synthetic source index\n\nUse list_source_files to discover the imported synthetic interview and registered repository snapshot. Read the returned paths before deriving claims; this index contains no source content.",
    )?;
    put_note(
        dest,
        "CONTEXT-MAP.md",
        json!({"type":"Reference","title":"Synthetic context map","language":"en"}),
        "# Synthetic context map\n\nNo prior project context has been copied. Define only contexts supported by the supplied synthetic sources.",
    )?;
    let mut count = 0usize;
    let mut bytes = 0usize;
    for directory in ["80_Templates", "tools/locales"] {
        let root = source.path(directory)?;
        for entry in walkdir::WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                !matches!(
                    e.file_name().to_str(),
                    Some("Baselines" | "Runs" | "Reports")
                )
            })
        {
            let entry = entry?;
            if entry.file_type().is_dir() {
                continue;
            }
            let path = entry
                .path()
                .strip_prefix(&source.root)?
                .to_str()
                .context("Non-UTF-8 seed path")?;
            if !(path.ends_with(".md")
                || path.starts_with("tools/locales/") && path.ends_with(".json"))
            {
                continue;
            }
            ensure!(!entry.file_type().is_symlink(), "Linked seed input: {path}");
            let content = read(&source.root, path)?;
            count += 1;
            bytes += content.len();
            ensure!(
                count <= 200 && bytes <= 8 * 1024 * 1024,
                "Framework seed exceeds template/catalog bounds"
            );
            put(dest, path, &content)?;
        }
    }
    for (_, skill, playbook) in crate::runtime::STAGES {
        let path = format!(".agents/skills/{skill}/SKILL.md");
        put(dest, &path, source.read(&path)?.as_bytes())?;
        if !playbook.is_empty() {
            let path = format!("90_Agent/Playbooks/{playbook}.md");
            put(dest, &path, source.read(&path)?.as_bytes())?;
        }
    }
    Ok(())
}

pub fn populate(root: &Path, w: &Workspace, case: &str, language: &str) -> Result<String> {
    let case = if case == "draft-stages" {
        "draft-chain"
    } else {
        case
    };
    put_note(
        w,
        "00_Governance/Project.md",
        json!({"type":"project","id":"PROJECT-EVAL","title":"Synthetic evaluation project","language":language,"workflow_status":"draft","owner":null}),
        if language == "zh-TW" {
            "# 合成評估專案\n\n僅供測試。所有角色、日期與數值均為虛構測試資料，不代表真實承諾。"
        } else {
            "# Synthetic evaluation project\n\nTest data only. All roles, dates and numbers are fictional fixture inputs, not real commitments."
        },
    )?;
    put_note(
        w,
        "00_Governance/Settings.md",
        json!({"type":"project_settings","title":"Evaluation language","language":"en","document_language":language,"agent_language":"en"}),
        "# Evaluation language\n\nSelected explicitly for this isolated synthetic case.",
    )?;
    put_note(
        w,
        "06_Delivery/Planning.md",
        json!({"type":"planning_config","id":"PLAN-CONFIG","title":"Synthetic planning","language":language,"as_of":null}),
        "# Synthetic planning\n\nNo dates have been supplied.",
    )?;
    put_note(
        w,
        "06_Delivery/People.md",
        json!({"type":"Reference","title":"Synthetic capacity","language":language}),
        "# Synthetic capacity\n\n| Person | Role | Sprint | Project | FTE | Workdays | Absence | Focus | Reserve |\n|---|---|---|---|---:|---:|---:|---:|---:|\n",
    )?;
    put_note(
        w,
        "06_Delivery/Iterations.md",
        json!({"type":"Reference","title":"Synthetic iterations","language":language}),
        "# Synthetic iterations\n\n| Sprint | Start | End | Goal |\n|---|---|---|---|\n",
    )?;
    let source = match case {
        "missing-inputs" => {
            "SYNTHETIC INTERVIEW, not a real project. Employees request temporary application access. The approval threshold, approver, duration, delivery deadline, team availability and effort estimates have not been supplied. These inputs are required before defining the approval behavior or scheduling delivery.\n"
        }
        "wiki-code-conflict" => {
            "SYNTHETIC CHANGE REQUEST, not a real project. Assess the current access-duration rule using both the registered OpenWiki and implementation snapshots. The desired future duration has not been decided. Neither source author has authority to choose a future business rule.\n"
        }
        "draft-chain" => {
            "SYNTHETIC REQUIREMENTS INTERVIEW, not a real project. The Access context handles employee access requests. Canonical term: Access request means a request by one employee for temporary access to one application. The current process uses email; the problem is missing observable request status. Proposed scope: employees submit a request with employee ID, application ID and duration_days; the manager decides approve or reject and the employee can view pending, approved or rejected. For this exercise the sponsor explicitly supplies duration_days as an integer from 1 through 7 inclusive; reject zero, negative, fractional or greater values before creating a request. The existing system observations are available in the registered repository snapshot. Excluded: automatic account provisioning, migration, notifications and changes to authentication. Draft a narrow first slice for duration validation. Numeric business benefits, owner names, effort, staffing, dates, NFR thresholds and integrations are not supplied; retain them as unknown and leave this PBI unscheduled. No approval, readiness, baseline or release is authorized. These are draft discovery records for later human review.\n"
        }
        "capacity-forecast" => {
            "SYNTHETIC CAPACITY EXERCISE, not a real project. Use only the supplied People, Iterations, Planning and PBI files. PBI-EVAL-UNKNOWN remains unscheduled and has no estimate. Evaluate capacity and identify any shortage or unknown without changing assignments, estimates, dates or delivery status. These numbers and dates are fictional test inputs.\n"
        }
        _ => anyhow::bail!("Unknown evaluation case"),
    };
    put(w, "01_Sources/Assets/interview.txt", source.as_bytes())?;
    crate::sources::ingest(w, "01_Sources/Assets/interview.txt")?;
    let wiki = if case == "wiki-code-conflict" {
        "# Synthetic access service\n\nThis sample OpenWiki claims that the maximum duration is 30 days. This is a documentation claim, not an approved future requirement.\n"
    } else {
        "# Synthetic access service\n\nThe sample service validates integer durations from 1 through 7 days. Existing implementation observations are not future business approval.\n"
    };
    let repo = root.join("repository");
    fs::create_dir_all(repo.join("openwiki"))?;
    fs::create_dir_all(repo.join("src"))?;
    fs::write(
        repo.join("openwiki/Access.md"),
        note(
            json!({"type":"Reference","title":"Synthetic access service","language":"en"}),
            wiki,
        )?,
    )?;
    fs::write(
        repo.join("src/access.rs"),
        "// Synthetic source fixture. Not a production repository.\npub fn allowed_duration(days: i64) -> bool { (1..=7).contains(&days) }\n",
    )?;
    crate::sources::add_repo(
        w,
        "EVAL",
        repo.to_str().context("Non-UTF-8 output path")?,
        "openwiki",
        &["src".into()],
    )?;
    crate::sources::sync(&crate::records::Records::load(w)?, "EVAL")?;
    if case == "capacity-forecast" {
        capacity(w, language)?;
    }
    let task = match case {
        "missing-inputs" => {
            "Read the supplied interview and inspect current planning inputs. Request the missing approval and planning information with an actual clarification question. Keep dependent requirements, PBIs and scheduling pending. Do not answer your own question."
        }
        "wiki-code-conflict" => {
            "Inspect the pinned OpenWiki AND source code, compare their duration rules, and request the business decision that is missing. Preserve both original sources and keep dependent requirements pending. Do not choose a future rule from either source alone."
        }
        "draft-chain" => {
            "請根據合成訪談及已註冊 Repository 的 OpenWiki 與程式碼快照，建立一個最小的領域詞彙庫、BRD、PRD、原子需求及 PBI 草稿鏈，聚焦 duration_days 驗證。使用專案 Settings 指定的文件語言，保留來源引述及版本依據、上下游連結和可檢查的驗收條件。未提供的估算、人力、時程及負責人保持未知；PBI 保持未排程。最後執行 validate 與 coverage 並如實回報缺口。不得將草稿宣告為核准或驗收通過。"
        }
        "capacity-forecast" => {
            "Inspect the supplied planning inputs and run the native plan operation. Explain the known role shortage and the unestimated unscheduled item. Preserve all supplied inputs and do not invent delivery dates or availability."
        }
        _ => unreachable!(),
    };
    if language == "zh-TW" {
        localize_fixture(w)?;
    }
    Ok(format!(
        "Synthetic evaluation only; do not contact external tools or change configuration. The interview is 01_Sources/Assets/interview.txt. Read it before the task; discover its imported copy/extraction and the REPO-EVAL snapshot files through list_source_files. The repository is a synthetic directory fixture, not an actual cloned or executed repository. No actual business owner, date or commitment is represented.\n\n{task}"
    ))
}

fn localize_fixture(w: &Workspace) -> Result<()> {
    // Preserve code, table headers, IDs, raw sources and copied English governance.
    // Only the fixture's authored project prose is translated here.
    let replacements = [
        ("Synthetic evaluation project", "合成評估專案"),
        ("Synthetic forecast date", "合成預測日期"),
        ("Synthetic planning", "合成規劃"),
        ("Synthetic capacity", "合成人力容量"),
        ("Synthetic iterations", "合成迭代"),
        ("Synthetic iteration", "合成迭代"),
        ("Synthetic work", "合成工作項目"),
        ("Synthetic exercise", "合成演練"),
        ("No dates have been supplied.", "未提供日期。"),
        (
            "Fictional fixture date, not today's date.",
            "此為虛構測試日期，並非今日日期。",
        ),
        (
            "Fictional planning input, not a ready item.",
            "此為虛構規劃輸入，工作項目尚未就緒。",
        ),
    ];
    for path in [
        "00_Governance/Project.md",
        "06_Delivery/Planning.md",
        "06_Delivery/People.md",
        "06_Delivery/Iterations.md",
        "05_Backlog/PBI-EVAL-KNOWN.md",
        "05_Backlog/PBI-EVAL-UNKNOWN.md",
    ] {
        if !w.path(path)?.is_file() {
            continue;
        }
        let mut text = w.read(path)?;
        for (en, zh) in replacements {
            text = text.replace(en, zh);
        }
        put(w, path, text.as_bytes())?;
    }
    Ok(())
}

fn capacity(w: &Workspace, language: &str) -> Result<()> {
    put_note(
        w,
        "06_Delivery/Planning.md",
        json!({"type":"planning_config","id":"PLAN-CONFIG","title":"Synthetic forecast date","language":language,"as_of":"2030-01-07"}),
        "# Synthetic forecast date\n\nFictional fixture date, not today's date.",
    )?;
    put_note(
        w,
        "06_Delivery/People.md",
        json!({"type":"Reference","title":"Synthetic capacity","language":language}),
        "# Synthetic capacity\n\n| Person | Role | Sprint | Project | FTE | Workdays | Absence | Focus | Reserve |\n|---|---|---|---|---:|---:|---:|---:|---:|\n| Fixture-BE | Backend | 1 | PROJECT-EVAL | 1 | 5 | 0 | 0.8 | 0.1 |\n| Fixture-QA | QA | 1 | PROJECT-EVAL | 0.5 | 5 | 0 | 0.8 | 0.1 |\n",
    )?;
    put_note(
        w,
        "06_Delivery/Iterations.md",
        json!({"type":"Reference","title":"Synthetic iteration","language":language}),
        "# Synthetic iteration\n\n| Sprint | Start | End | Goal |\n|---|---|---|---|\n| 1 | 2030-01-07 | 2030-01-11 | Synthetic exercise |\n",
    )?;
    for (id, sprint, rows) in [
        (
            "PBI-EVAL-KNOWN",
            json!(1),
            "| Backend | 4 | 4 | 4 |\n| QA | 1 | 1 | 1 |\n",
        ),
        ("PBI-EVAL-UNKNOWN", Value::Null, ""),
    ] {
        put_note(
            w,
            &format!("05_Backlog/{id}.md"),
            json!({"type":"pbi","id":id,"title":"Synthetic work","language":language,"doc_status":"draft","delivery_status":"proposed","work_type":"enabler","sprint":sprint,"depends_on":[],"owner":null}),
            &format!(
                "# Synthetic work\n\nFictional planning input, not a ready item.\n\n| Role | O | M | P |\n|---|---:|---:|---:|\n{rows}"
            ),
        )?;
    }
    Ok(())
}
