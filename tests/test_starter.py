"""Check the reusable distribution independently of generated project records."""
import re
import sys
import tempfile
import unittest
from pathlib import Path
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'tools'))
from pmkit import governance, okf, planning, quality, settings
from pmkit.vault import read_note, resolve_link, save_note
from support import copy_starter


class StarterTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='pm-starter-')
        self.root = (Path(self.temp.name)/'vault').resolve()
        copy_starter(self.root)

    def tearDown(self):
        self.temp.cleanup()

    def test_starter_has_no_demo_records(self):
        for folder in ['01_Sources/Assets','01_Sources/Extracted','01_Sources/Notes',
                       '01_Sources/Evidence','03_Systems/Repositories','03_Systems/Analysis',
                       '03_Systems/Snapshots','04_Requirements','05_Backlog','07_Decisions',
                       '08_Verification/Cases','90_Agent/Runs','90_Agent/Reports',
                       '00_Governance/Baselines']:
            self.assertEqual([p for p in (self.root/folder).rglob('*')
                              if p.is_file() and p.name!='.gitkeep'], [], folder)
        self.assertFalse((self.root/'samples').exists())
        self.assertFalse((self.root/'06_Delivery/Plan-Report.md').exists())

    def test_framework_names_and_prose_are_english(self):
        for p in self.root.rglob('*'):
            self.assertTrue(p.relative_to(self.root).as_posix().isascii(), str(p))
            if p.suffix in {'.md','.canvas'}:
                self.assertIsNone(re.search(r'[\u3400-\u9fff]',p.read_text()),str(p))
            if p.suffix=='.md' and p.name not in {'SKILL.md','index.md','log.md'}:
                self.assertEqual(read_note(p)[0].get('language'),'en',str(p))
        self.assertEqual(settings.load(self.root)['document_language'],'en')

    def test_project_language_does_not_change_framework_language(self):
        settings.update(self.root,'zh-TW')
        for path in ['README.md','HOME.md','00_Governance/Project-Setup.md',
                     '90_Agent/Working-with-Agents.md','.agents/skills/pm-prd/SKILL.md',
                     '80_Templates/BRD.md','03_Systems/OpenWiki-Integration.md']:
            self.assertEqual(settings.required_language(self.root,path),'en',path)
        self.assertEqual(settings.required_language(self.root,'04_Requirements/BRD/BRD-NEW.md'),'zh-TW')

    def test_fresh_validation_reports_setup_without_structural_errors(self):
        findings=quality.inspect(self.root)
        self.assertEqual([f for f in findings if f['level']=='ERROR'],[])
        self.assertEqual({f['code'] for f in findings},{'PROJECT_SETUP'})
        _,failures=quality.write_report(self.root,strict=True)
        self.assertGreater(failures,0)

    def test_unconfigured_planning_is_blocked_not_zero(self):
        with self.assertRaisesRegex(ValueError,'not configured'):
            planning.write_plan(self.root)
        meta,body=read_note(self.root/'06_Delivery/Plan-Report.md')
        self.assertEqual(meta['workflow_status'],'blocked')
        self.assertNotIn('Scheduled remaining effort',body)

    def test_periods_require_an_explicit_forecast_date(self):
        p=self.root/'06_Delivery/Iterations.md';m,b=read_note(p)
        save_note(p,m,'# Iterations\n\n| Sprint | Start | End | Goal |\n|---|---|---|---|\n| 1 | 2030-01-01 | 2030-01-07 | Test |')
        with self.assertRaisesRegex(ValueError,'set as_of'):
            planning.calculate(self.root)

    def test_status_accepts_empty_project_without_fabricating_date(self):
        _,body=read_note(governance.status_report(self.root))
        self.assertIn('not configured',body)
        self.assertIn('No backlog items recorded.',body)
        self.assertNotIn('[[06_Delivery/Plan-Report]]',body)

    def test_document_and_canvas_links_resolve(self):
        for p in self.root.rglob('*'):
            if p.suffix not in {'.md','.canvas'}: continue
            body=re.sub(r'```.*?```|~~~.*?~~~','',p.read_text(),flags=re.S)
            for target in re.findall(r'\[\[([^\]]+)\]\]',body):
                with self.subTest(file=p.relative_to(self.root),target=target):
                    self.assertTrue(resolve_link(self.root,target).is_file())
            if p.suffix=='.canvas': continue
            for target in re.findall(r'(?<!!)\[[^\]]+\]\(([^)]+)\)',body):
                target=unquote(target)
                if urlsplit(target).scheme or target.startswith('#'): continue
                with self.subTest(file=p.relative_to(self.root),target=target):
                    self.assertTrue((p.parent/target.split('#')[0]).is_file())

    def test_clean_okf_export_conforms_without_project_examples(self):
        target=Path(self.temp.name)/'bundle'
        result=okf.export(self.root,target)
        self.assertTrue(result['conformant'])
        self.assertFalse((target/'samples').exists())
        self.assertEqual(len(list((target/'90_Agent/Skills').glob('*.md'))),9)
        for p in target.rglob('*.md'):
            body=re.sub(r'```.*?```','',p.read_text(),flags=re.S)
            self.assertNotIn('[[',body,str(p))


if __name__=='__main__': unittest.main()
