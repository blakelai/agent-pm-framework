from __future__ import annotations

import base64
import importlib.util
import json
import shutil
import subprocess
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

VAULT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(VAULT/'tools'))
import pm
from pmkit import sources, quality, planning
from pmkit.vault import digest, read_note, save_note

from support import build_fixture


class FrameworkTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='agent-pm-test-')
        self.root = (Path(self.temp.name)/'vault').resolve()
        build_fixture(self.root)

    def tearDown(self):
        self.temp.cleanup()

    def codes(self):
        return {x['code'] for x in quality.inspect(self.root)}

    def test_fixture_has_no_structural_errors(self):
        errors = [x for x in quality.inspect(self.root) if x['level']=='ERROR']
        self.assertEqual(errors, [])
        self.assertIn('DRAFT', self.codes())

    def test_obsidian_block_list_properties(self):
        path=self.root/'scratch.md'
        path.write_text('---\nid: NOTE-1\ntype: Note\ncontexts:\n  - alpha\nparents:\n  - "[[04_Requirements/BRD/BRD-001]]"\nflag: false\nvalue: 0\n---\n# Note\n',encoding='utf-8')
        meta,_=read_note(path)
        self.assertEqual(meta['contexts'],['alpha'])
        self.assertEqual(meta['value'],0)
        self.assertFalse(meta['flag'])
        save_note(path,meta,'# Changed')
        after,_=read_note(path)
        self.assertEqual({key:after[key] for key in meta},meta)
        self.assertEqual(after['title'],'Changed')

    def test_duplicate_properties_fail_loudly(self):
        path=self.root/'scratch.md'
        path.write_text('---\nid: A\nid: B\n---\n',encoding='utf-8')
        with self.assertRaises(ValueError):read_note(path)

    def test_ingestion_is_idempotent_and_keeps_edits(self):
        file=Path(self.temp.name)/'new.txt';file.write_text('New requirement',encoding='utf-8')
        note=sources.ingest(self.root,file)
        meta,body=read_note(note);save_note(note,meta,body+'\nHuman annotation')
        self.assertEqual(sources.ingest(self.root,file),note)
        self.assertIn('Human annotation',read_note(note)[1])
        self.assertEqual((self.root/meta['asset']).read_bytes(),file.read_bytes())

    def test_images_are_pending_visual_review(self):
        file=Path(self.temp.name)/'pixel.png'
        file.write_bytes(base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a6WQAAAAASUVORK5CYII='))
        meta,body=read_note(sources.ingest(self.root,file))
        self.assertEqual(meta['extraction_status'],'visual_review_required')
        self.assertEqual(meta['review_status'],'pending')
        self.assertIn('![[01_Sources/Assets/',body)

    def test_pptx_text_preserves_slide_numbers(self):
        file=Path(self.temp.name)/'slides.pptx'
        with zipfile.ZipFile(file,'w') as z:
            for n in [2,1]:
                z.writestr(f'ppt/slides/slide{n}.xml',f'<root xmlns:a="urn:test"><a:t>Slide {n}</a:t></root>')
        chunks,status=sources.extract(file)
        self.assertEqual(chunks,[('slide-1','Slide 1'),('slide-2','Slide 2')])
        self.assertEqual(status,'visual_review_required')

    def test_docx_text_is_not_claimed_as_full_layout(self):
        file=Path(self.temp.name)/'document.docx'
        with zipfile.ZipFile(file,'w') as z:
            z.writestr('word/document.xml','<root xmlns:w="urn:test"><w:t>Hello</w:t></root>')
        chunks,status=sources.extract(file)
        self.assertEqual(chunks,[('document-body','Hello')])
        self.assertEqual(status,'visual_review_required')

    @unittest.skipUnless(importlib.util.find_spec('pypdf') and importlib.util.find_spec('reportlab'),'optional PDF test dependencies')
    def test_pdf_page_locator(self):
        from reportlab.pdfgen import canvas
        file=Path(self.temp.name)/'document.pdf'
        c=canvas.Canvas(str(file));c.drawString(50,700,'Page one');c.showPage();c.drawString(50,700,'Page two');c.save()
        chunks,status=sources.extract(file)
        self.assertEqual([x[0] for x in chunks],['page-1','page-2'])
        self.assertIn('Page two',chunks[1][1])
        self.assertEqual(status,'visual_review_required')

    def test_snapshot_is_read_only_idempotent_and_preserves_claims(self):
        repo=self.root/'01_Sources/Assets/test-repository'
        before={p.relative_to(repo).as_posix():digest(p.read_bytes()) for p in repo.rglob('*') if p.is_file()}
        first=sources.sync_repo(self.root,'test-service');second=sources.sync_repo(self.root,'test-service')
        self.assertEqual(first,second)
        self.assertEqual(before,{p.relative_to(repo).as_posix():digest(p.read_bytes()) for p in repo.rglob('*') if p.is_file()})
        self.assertEqual((first.parent/'files/openwiki/.claims/fixture.json').read_bytes(),(repo/'openwiki/.claims/fixture.json').read_bytes())

    def test_git_revision_and_dirty_state_are_recorded(self):
        repo=self.root/'01_Sources/Assets/test-repository'
        def run(*args):subprocess.run(['git','-C',str(repo),*args],check=True,capture_output=True)
        run('init');run('add','.');run('-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','-m','test fixture')
        first=sources.sync_repo(self.root,'test-service')
        first_meta=json.loads((first.parent/'manifest.json').read_text())
        self.assertFalse(first_meta['dirty']);self.assertNotEqual(first_meta['revision'],'unversioned')
        code=repo/'src/handler.py';code.write_text(code.read_text()+'\n# edit\n')
        second=sources.sync_repo(self.root,'test-service')
        second_meta=json.loads((second.parent/'manifest.json').read_text())
        self.assertTrue(second_meta['dirty']);self.assertNotEqual(first,second)
        self.assertEqual(first_meta['revision'],second_meta['revision'])

    def test_source_change_is_reported_without_overwriting_evidence(self):
        evidence=self.root/'01_Sources/Evidence/EVD-CODE-001.md';old=evidence.read_bytes()
        code=self.root/'01_Sources/Assets/test-repository/src/handler.py';code.write_text(code.read_text()+'\n# new version\n')
        self.assertIn('STALE_EVIDENCE',self.codes());self.assertEqual(evidence.read_bytes(),old)

    def test_ignore_rules_and_secret_paths(self):
        repo=self.root/'01_Sources/Assets/test-repository'
        (repo/'.openwikiignore').write_text('src/private.py\n')
        (repo/'src/private.py').write_text('private fixture')
        (repo/'src/.env').write_text('fixture only')
        snap=sources.sync_repo(self.root,'test-service')
        files=json.loads((snap.parent/'manifest.json').read_text())['files']
        self.assertNotIn('src/private.py',files);self.assertNotIn('src/.env',files)
        (repo/'.openwikiignore').write_text('!src/private.py\n')
        with self.assertRaises(ValueError):sources.sync_repo(self.root,'test-service')

    def test_path_escape_and_invalid_line_range_are_rejected(self):
        with self.assertRaises(ValueError):sources.add_repo(self.root,'bad','01_Sources/Assets/test-repository','../../../../')
        with self.assertRaises(ValueError):sources.capture_evidence(self.root,'EVD-BAD','claim',repo_id='test-service',path='src/handler.py',start=0,end=999)

    def test_terminology_checks_authored_text_not_quotes(self):
        path=self.root/'04_Requirements/BRD/BRD-001.md';meta,body=read_note(path)
        save_note(path,meta,body+'\n> 原文：舊稱\n\n`舊稱`\n')
        self.assertNotIn('TERMINOLOGY',self.codes())
        save_note(path,meta,body+'\n需要新增舊稱狀態。\n')
        self.assertIn('TERMINOLOGY',self.codes())

    def test_approval_cannot_pass_without_upstream_and_evidence(self):
        path=self.root/'05_Backlog/PBI-001.md';meta,body=read_note(path);meta['delivery_status']='ready';save_note(path,meta,body)
        self.assertTrue({'READINESS_RECORD','EXECUTION_BASELINE'} <= self.codes())

    def test_broken_links_and_duplicate_ids(self):
        path=self.root/'04_Requirements/BRD/extra.md'
        save_note(path,{'id':'BRD-001','type':'brd'},'[[missing-file]]')
        self.assertIn('LINK',self.codes());self.assertIn('DUPLICATE_ID',self.codes())

    def test_effort_capacity_and_zero_capacity_recalculate(self):
        plan=planning.calculate(self.root);caps=plan['caps'];totals=plan['totals']
        self.assertAlmostEqual(totals['PBI-001'],8.5*6.25/6)
        self.assertAlmostEqual(caps[(1,'Backend')]['capacity'],7.2)
        path=self.root/'06_Delivery/People.md';text=path.read_text()
        path.write_text(text.replace('| test-Backend | Backend | 1 | PROJECT-TEST | 1 |','| test-Backend | Backend | 1 | PROJECT-TEST | 0 |'))
        caps=planning.calculate(self.root)['caps']
        self.assertEqual(caps[(1,'Backend')]['capacity'],0)
        self.assertGreater(caps[(1,'Backend')]['effort'],0)

    def test_missing_estimate_blocks_instead_of_looking_like_zero(self):
        path=self.root/'05_Backlog/PBI-001.md';text=path.read_text()
        path.write_text(text.replace('| BA | 0.375 | 0.5 | 0.75 |','| BA | | 0.5 | 0.75 |'))
        with self.assertRaises(ValueError):planning.write_plan(self.root)
        meta,body=read_note(self.root/'06_Delivery/Plan-Report.md')
        self.assertEqual(meta['workflow_status'],'blocked');self.assertNotIn('總工作量',body)

    def test_cyclic_dependencies_are_detected(self):
        path=self.root/'05_Backlog/PBI-001.md';meta,body=read_note(path)
        meta['depends_on']=['[[05_Backlog/PBI-002]]'];save_note(path,meta,body)
        with self.assertRaisesRegex(ValueError,'Cyclic'):planning.calculate(self.root)

    def test_agent_packet_is_prepared_not_falsely_completed(self):
        path=pm.prepare(self.root,'prd',['BRD-001'])
        meta,body=read_note(path)
        self.assertEqual(meta['workflow_status'],'prepared');self.assertIn('BRD-001',body)
        self.assertIn('has not executed',body)


if __name__ == '__main__':unittest.main()
