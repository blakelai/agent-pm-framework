#!/usr/bin/env bash
set -euo pipefail
if [ "$#" -ne 3 ]; then
  printf '%s\n' 'Usage: bash setup.sh /path/to/pm-agent /path/to/clean-starter /new/output-directory' >&2
  exit 2
fi

sample=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
binary_dir=$(CDPATH= cd -- "$(dirname -- "$1")" && pwd -P)
binary="$binary_dir/$(basename -- "$1")"
starter=$(CDPATH= cd -- "$2" && pwd -P)
output_parent=$(CDPATH= cd -- "$(dirname -- "$3")" && pwd -P)
output="$output_parent/$(basename -- "$3")"

if [ ! -x "$binary" ] || [ ! -f "$starter/00_Governance/Settings.md" ] ||
    [ -e "$starter/.git" ] || [ -e "$starter/.pm-agent" ] ||
    ! grep -qx 'workflow_status: "unconfigured"' "$starter/00_Governance/Project.md"; then
  printf '%s\n' 'Use an executable pm-agent and a clean starter from a native package.' >&2
  exit 2
fi
case "$output/" in
  "$starter/"*|"$sample/"*) printf '%s\n' 'Choose an output outside the starter and example source.' >&2; exit 2 ;;
esac
command -v git >/dev/null
command -v python3 >/dev/null

umask 077
mkdir -- "$output"
mkdir -- "$output/checks" "$output/vault"
cp -R "$starter/." "$output/vault/"
cp -R "$sample/operator" "$output/operator"
repository="$output/repository"
vault="$output/vault"
checks="$output/checks"

copy_inputs() {
  local source=$1 destination=$2 file relative target
  while IFS= read -r -d '' file; do
    relative=${file#"$source/"}
    target="$destination/${relative%.in}"
    mkdir -p -- "$(dirname -- "$target")"
    cp -- "$file" "$target"
  done < <(find "$source" -type f \( -name '*.md.in' -o -name '*.py' \) -print0)
}

copy_inputs "$sample/repository" "$repository"
git -C "$repository" -c init.defaultBranch=main -c init.templateDir= init --quiet
git -C "$repository" add -- openwiki src tests
GIT_AUTHOR_DATE=2001-01-01T00:00:00Z GIT_COMMITTER_DATE=2001-01-01T00:00:00Z \
  git -C "$repository" -c core.hooksPath=/dev/null -c commit.gpgSign=false \
  -c user.name='Synthetic PM Agent Pilot' -c user.email='pilot@example.invalid' \
  commit --quiet --message 'Capture synthetic purchase routing'
revision=$(git -C "$repository" rev-parse HEAD)
(cd -- "$repository" && python3 -B -m unittest discover -s tests -v) > "$checks/repository-tests.log" 2>&1

pm() { "$binary" --vault "$vault" pm "$@"; }
pm settings --document-language zh-TW > "$checks/settings.json"
copy_inputs "$sample/vault" "$vault"
pm ingest "$sample/materials/intake.txt" > "$checks/ingest.json"
source_id=$(python3 -c 'import json, pathlib, sys; print(pathlib.Path(json.load(sys.stdin)["reports"][0]["path"]).stem)' < "$checks/ingest.json")
pm repo-add --id PILOT-PROCUREMENT --path "$repository" --wiki openwiki --code-root src --code-root tests > "$checks/repo-add.json"
pm sync --repo PILOT-PROCUREMENT > "$checks/sync.json"
pm evidence --id EVD-PILOT-INTAKE --source "$source_id" --locator '合成訪談第 2 行' \
  --claim '每張設備請購單記錄請購單號、申請部門及以整數新臺幣表示的含稅總額。' > "$checks/evidence-intake.json"
pm evidence --id EVD-PILOT-WIKI --repo PILOT-PROCUREMENT --path openwiki/Approval-Routing.md \
  --start 9 --end 11 --claim 'OpenWiki 記載含稅總額高於新臺幣 100,000 元時須二次核准。' > "$checks/evidence-wiki.json"
pm evidence --id EVD-PILOT-CODE --repo PILOT-PROCUREMENT --path src/procurement.py \
  --start 3 --end 10 --claim '此程式碼在金額高於新臺幣 50,000 元時回傳兩個核准角色。' > "$checks/evidence-code.json"

pm validate > "$checks/validate.json"
pm coverage > "$checks/coverage.json"
pm impact REQ-PILOT-INTAKE > "$checks/impact.json"
pm questions > "$checks/questions.json"
mkdir -p "$vault/90_Agent/Runs"
cat > "$vault/90_Agent/Runs/RUN-PILOT-SETUP.md" <<RUN
---
type: "agent_run"
id: "RUN-PILOT-SETUP"
language: "en"
title: "Prepare the synthetic procurement pilot"
workflow_status: "completed"
---

# Prepare the synthetic procurement pilot

Created a separate repository and project Vault at $(date -u +%FT%TZ).
Project language is zh-TW and agent records use English. Setup used repository
revision $revision, interview $source_id and the starter supplied by the operator.

Created the charter, context, source notes and evidence, system analysis, intake
BRD, PRD, requirement, proposed PBI and test design. Captured the OpenWiki and
code with separate evidence locators. Repository unit tests, native validation,
coverage and impact ran. The sibling checks directory contains the actual results.

The open routing question requests a future threshold, equality behavior, approver
and order. The synthetic answer is held in the sibling operator directory for later
submission. The draft routing chain waits for that answer. Real pilot approval,
independent review, intake test execution and release are pending.
RUN
pm validate > "$checks/validate.json"

printf 'Vault: %s\nRepository: %s\nRevision: %s\nChecks: %s\n' "$vault" "$repository" "$revision" "$checks"
