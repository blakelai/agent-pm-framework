import importlib.util
import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'tools'))
from pmkit import baselines, quality, planning, traceability
from pmkit.vault import read_note, save_note, by_id, catalog, content_hash, digest

from support import build_fixture


class GovernanceTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix='pm-governance-test-')
        self.root=(Path(self.temp.name)/'vault').resolve()
        build_fixture(self.root)

    def tearDown(self):self.temp.cleanup()

    def edit(self,id_,**updates):
        p,m,b=by_id(self.root,id_);m.update(updates);save_note(p,m,b);return p,m,b

    def codes(self):return {f['code'] for f in quality.inspect(self.root)}

    def candidate(self):
        ids=[m['id'] for _,m,_ in catalog(self.root) if m.get('type') in baselines.DOCUMENT_TYPES]
        path=baselines.capture(self.root,'BASE-TEST',ids)
        return path

    def approved_fixture(self):
        path=self.candidate()
        # Synthetic decision for a unit test only; never copied into delivered project data.
        save_note(self.root/'07_Decisions/DEC-TEST.md',{'id':'DEC-TEST','type':'decision','decision_type':'baseline_approval','baseline_id':'BASE-TEST','baseline_sha256':digest(path.read_bytes()),'workflow_status':'accepted','decided_by':'unit-test fixture actor','decided_at':'2026-10-09T08:00:00+08:00','decision_evidence':['[[00_Governance/Project]]']},'# Synthetic test fixture')

    def run_record(self,id_='TR-TEST',result='passed',at='2026-10-09T09:00:00+08:00',simulation=False):
        _,m,b=by_id(self.root,'REQ-001')
        token=m['id']+'@'+str(m['version'])+':'+content_hash(m,b)
        save_note(self.root/'08_Verification'/f'{id_}.md',{'id':id_,'type':'test_run','result':result,'cases':['[[08_Verification/Cases/TC-001]]'],'requirement_versions':[token],'executed_at':at,'executed_by':'fixture','environment':'isolated-test','build':'fixture-sha','evidence':['[[00_Governance/Project]]'],'simulation':simulation},'# Synthetic test result fixture')

    def test_baseline_capture_does_not_approve_or_modify_source(self):
        p,_,_=by_id(self.root,'REQ-001');before=p.read_bytes();self.candidate()
        self.assertEqual(p.read_bytes(),before)
        with self.assertRaisesRegex(ValueError,'No accepted'):baselines.approval(self.root,'BASE-TEST')
        self.assertEqual(read_note(p)[0]['doc_status'],'draft')

    def test_baseline_id_cannot_be_overwritten(self):
        self.candidate()
        with self.assertRaises(ValueError):self.candidate()

    def test_snapshot_tampering_is_detected(self):
        self.candidate();p=self.root/'00_Governance/Baselines/BASE-TEST/files/REQ-001.md'
        p.write_text(p.read_text()+'tamper')
        self.assertIn('BASELINE_INTEGRITY',self.codes())

    def test_approved_content_change_invalidates_current_document(self):
        self.approved_fixture()
        p,m,b=self.edit('REQ-001',doc_status='approved',baseline_id='BASE-TEST')
        self.assertNotIn('BASELINE_INVALID',self.codes())
        save_note(p,m,b+'\nChanged business rule')
        self.assertIn('BASELINE_INVALID',self.codes())

    def test_workflow_annotations_do_not_change_approved_hash(self):
        _,m,b=by_id(self.root,'REQ-001');old=content_hash(m,b)
        m.update(doc_status='approved',baseline_id='BASE-TEST',reviewed_at='2026-10-09')
        self.assertEqual(content_hash(m,b),old)
        m['version']=2;self.assertNotEqual(content_hash(m,b),old)

    def test_new_draft_preserves_old_execution_baseline(self):
        self.approved_fixture()
        self.edit('PBI-001',delivery_status='ready',baseline_id='BASE-TEST',ready_by='fixture',ready_at='2026-10-09',readiness_evidence=['[[00_Governance/Project]]'])
        p,m,b=self.edit('REQ-001',version=2,doc_status='draft');save_note(p,m,b+'\nNew proposal')
        self.assertIn('NEWER_DRAFT',self.codes());self.assertNotIn('EXECUTION_BASELINE',self.codes())
        pinned,_=baselines.pinned_document(self.root,'BASE-TEST','REQ-001')
        self.assertEqual(pinned['version'],1)

    def test_decision_for_wrong_manifest_hash_does_not_approve(self):
        self.approved_fixture();self.edit('DEC-TEST',baseline_sha256='wrong')
        with self.assertRaises(ValueError):baselines.approval(self.root,'BASE-TEST')

    def test_simulated_approval_is_not_formal_approval(self):
        self.approved_fixture();self.edit('DEC-TEST',simulation=True)
        with self.assertRaises(ValueError):baselines.approval(self.root,'BASE-TEST')

    def test_rescinded_approval_is_not_valid(self):
        self.approved_fixture();self.edit('DEC-TEST',workflow_status='rescinded')
        with self.assertRaises(ValueError):baselines.approval(self.root,'BASE-TEST')

    def test_impact_follows_requirements_pbis_and_cases(self):
        p=baselines.impact(self.root,'PRD-001');text=p.read_text()
        self.assertIn('REQ-001',text);self.assertIn('PBI-001',text);self.assertIn('TC-001',text)

    def test_unscheduled_unknown_does_not_block_scheduled_forecast(self):
        plan=planning.calculate(self.root)
        self.assertIn('PBI-003',plan['unscheduled']);self.assertIsNone(plan['totals']['PBI-003'])
        self.assertGreater(plan['caps'][(1,'Backend')]['effort'],0)

    def test_empty_schedule_is_not_presented_as_delivery_plan(self):
        self.edit('PBI-001',sprint=None);self.edit('PBI-002',sprint=None)
        report=planning.write_plan(self.root).read_text()
        self.assertIn('不構成交付計畫',report)
        self.assertIn('未估算的工作量不是零',report)

    def test_multi_context_terms_require_review_without_global_replacement(self):
        p,m,b=self.edit('PRD-001',contexts=['alpha','beta'])
        save_note(p,m,b+'\n測試詞在不同語境需要個別審查。')
        findings=[f for f in quality.inspect(self.root) if f['file']=='04_Requirements/PRD/PRD-001.md']
        self.assertEqual(sum(f['code']=='MULTI_CONTEXT_REVIEW' for f in findings),1)
        self.assertFalse(any(f['code']=='TERMINOLOGY' for f in findings))

    def test_done_and_cancelled_do_not_consume_future_capacity(self):
        self.edit('PBI-001',delivery_status='done');self.edit('PBI-002',delivery_status='cancelled')
        plan=planning.calculate(self.root)
        self.assertEqual(sum(c['effort'] for c in plan['caps'].values()),0)

    def test_in_progress_requires_remaining_estimate(self):
        self.edit('PBI-001',delivery_status='in_progress')
        with self.assertRaisesRegex(ValueError,'Remaining'):planning.calculate(self.root)

    def test_remaining_estimate_replaces_original_for_forecast(self):
        p,m,b=self.edit('PBI-001',delivery_status='in_progress')
        save_note(p,m,b+'\n\n| Role | Remaining O | Remaining M | Remaining P |\n|---|---:|---:|---:|\n| Backend | 1 | 1 | 1 |\n')
        plan=planning.calculate(self.root)
        self.assertEqual(plan['totals']['PBI-001'],1)
        self.assertGreater(plan['original']['PBI-001'],1)

    def test_cross_project_person_overallocation_is_blocked(self):
        p=self.root/'06_Delivery/People.md';p.write_text(p.read_text().rstrip()+'\n| test-Backend | Support | 1 | OTHER | 0.1 | 10 | 0 | 1 | 0 |\n')
        with self.assertRaisesRegex(ValueError,'overallocated'):planning.calculate(self.root)

    def test_external_wait_is_not_hidden_by_spare_capacity(self):
        self.edit('PBI-001',ready_after='2026-11-01')
        plan=planning.calculate(self.root);self.assertTrue(any('交期不可行' in s for s in plan['messages']))

    def test_pending_dependency_is_reported(self):
        self.edit('PBI-001',depends_on=['[[05_Backlog/PBI-003]]'])
        self.assertTrue(any('尚未排程' in s for s in planning.calculate(self.root)['messages']))

    def test_past_unfinished_work_requires_reschedule(self):
        self.edit('PLAN-CONFIG',as_of='2026-11-07')
        p=self.root/'06_Delivery/People.md';p.write_text(p.read_text().replace('| 10 | 0 |','| 0 | 0 |'))
        with self.assertRaisesRegex(ValueError,'past Sprint'):planning.calculate(self.root)

    def test_spike_does_not_require_fabricated_prd(self):
        _,m,_=by_id(self.root,'PBI-003');self.assertNotIn('parents',m)
        codes=[f for f in quality.inspect(self.root) if f['file']=='05_Backlog/PBI-003.md' and f['level']=='ERROR']
        self.assertEqual(codes,[])
        self.edit('PBI-003',timebox_days=0);self.assertIn('SPIKE_CONTRACT',self.codes())

    def test_test_case_does_not_count_as_executed_result(self):
        req=by_id(self.root,'REQ-001');_,cases,runs,releases,missing=traceability.evidence_for(self.root,req,catalog(self.root))
        self.assertEqual(len(cases),1);self.assertEqual(runs,[]);self.assertEqual(releases,[]);self.assertEqual(missing,{'TC-001'})

    def test_current_hash_and_version_are_required_for_test_coverage(self):
        self.run_record();req=by_id(self.root,'REQ-001')
        self.assertEqual(len(traceability.evidence_for(self.root,req,catalog(self.root))[2]),1)
        p,m,b=self.edit('REQ-001',version=2);save_note(p,m,b+'\nChanged rule')
        self.assertEqual(traceability.evidence_for(self.root,by_id(self.root,'REQ-001'),catalog(self.root))[2],[])

    def test_later_failed_result_supersedes_previous_pass(self):
        self.run_record();self.run_record('TR-LATER','failed','2026-10-09T10:00:00+08:00')
        self.assertEqual(traceability.evidence_for(self.root,by_id(self.root,'REQ-001'),catalog(self.root))[2],[])

    def test_simulated_test_results_are_not_formal_passes(self):
        self.run_record(simulation=True)
        self.assertEqual(traceability.evidence_for(self.root,by_id(self.root,'REQ-001'),catalog(self.root))[2],[])

    def test_done_feature_without_valid_test_results_fails(self):
        self.approved_fixture()
        self.edit('PBI-001',delivery_status='done',baseline_id='BASE-TEST',ready_by='fixture',ready_at='2026-10-09',readiness_evidence=['[[00_Governance/Project]]'],ac_coverage=['AC-01=TC-001','AC-02=TC-003'],acceptance_evidence=['[[00_Governance/Project]]'],dod_evidence=['[[00_Governance/Project]]'])
        self.assertIn('DONE_TEST_RESULTS',self.codes())


if __name__=='__main__':unittest.main()
