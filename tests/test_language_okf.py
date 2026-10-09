import importlib.util
import json
import shutil
import sys
import tempfile
import unittest
import subprocess
from pathlib import Path

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'tools'))
from pmkit import settings, planning, sources, quality, okf
from pmkit.vault import read_note, save_note, digest, resolve_link

from support import build_fixture


class LanguageAndOKFTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix='pm-language-okf-')
        self.root=(Path(self.temp.name)/'vault').resolve()
        build_fixture(self.root)

    def tearDown(self): self.temp.cleanup()

    def test_setting_change_preserves_existing_documents_and_source_bytes(self):
        before={p.relative_to(self.root):p.read_bytes() for p in self.root.rglob('*') if p.is_file() and p.relative_to(self.root).as_posix()!=settings.SETTINGS_PATH}
        settings.update(self.root,'en-us')
        self.assertEqual(settings.load(self.root)['document_language'],'en-US')
        self.assertEqual(before,{rel:(self.root/rel).read_bytes() for rel in before})

    def test_wiki_link_preserves_dots_in_document_name(self):
        path=self.root/'00_Governance/Version-2.1.md'
        save_note(path,{'type':'Reference'},'# A versioned filename')
        self.assertEqual(resolve_link(self.root,'00_Governance/Version-2.1'),path.resolve())

    def test_status_command_updates_status_report_without_recalculating_plan(self):
        plan=self.root/'06_Delivery/Plan-Report.md';before=plan.read_bytes()
        status=self.root/'90_Agent/Reports/Status-Report.md';status.unlink()
        result=subprocess.run([sys.executable,str(self.root/'tools/pm.py'),'status'],capture_output=True,text=True)
        self.assertEqual(result.returncode,0,result.stderr)
        meta,body=read_note(status)
        self.assertEqual(meta['id'],'REPORT-STATUS')
        self.assertEqual(meta['language'],'en')
        self.assertIn('Project status',body)
        self.assertEqual(plan.read_bytes(),before)

    def test_project_report_changes_language_without_changing_capacity(self):
        original=planning.calculate(self.root)
        settings.update(self.root,'en')
        current=planning.calculate(self.root)
        self.assertEqual(original['caps'],current['caps'])
        self.assertEqual(original['totals'],current['totals'])
        meta,body=read_note(planning.write_plan(self.root))
        self.assertEqual(meta['language'],'en')
        self.assertIn('Scheduled remaining effort',body)
        self.assertNotIn('人力與交付預測',body)

    def test_agent_reports_remain_english_under_chinese_project_setting(self):
        path,_=quality.write_report(self.root)
        meta,body=read_note(path)
        self.assertEqual(meta['language'],'en')
        self.assertIn('Document quality report',body)
        self.assertEqual(settings.required_language(self.root,'90_Agent/Runs/new.md'),'en')
        self.assertEqual(settings.required_language(self.root,'04_Requirements/Items/new.md'),'zh-TW')

    def test_prepared_packet_carries_non_default_language_without_renderer(self):
        settings.update(self.root,'ja-JP')
        spec=importlib.util.spec_from_file_location('pm_language_test',ROOT/'tools/pm.py')
        module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
        meta,body=read_note(module.prepare(self.root,'prd',['BRD-001']))
        self.assertEqual(meta['workflow_status'],'prepared')
        self.assertEqual(meta['language'],'en')
        self.assertIn('Project document language: ja-JP',body)

    def test_missing_renderer_does_not_overwrite_prior_forecast(self):
        path=planning.write_plan(self.root);before=path.read_bytes()
        settings.update(self.root,'ja-JP')
        with self.assertRaisesRegex(ValueError,'No project-document renderer'):planning.write_plan(self.root)
        self.assertEqual(before,path.read_bytes())

    def test_incomplete_or_corrupted_catalog_is_not_published(self):
        settings.update(self.root,'ja-JP')
        path=self.root/'tools/locales/ja-JP.json'
        path.write_text('{}')
        with self.assertRaisesRegex(ValueError,'exactly'):settings.catalog(self.root)
        data=json.loads((self.root/'tools/locales/en.json').read_text())
        data['plan_intro']='{invented}'
        path.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError,'placeholders'):settings.catalog(self.root)

    def test_valid_custom_catalog_is_used(self):
        # Synthetic strings test catalog routing, not Japanese linguistic quality.
        data=json.loads((self.root/'tools/locales/en.json').read_text());data['plan_title']='計画テスト'
        (self.root/'tools/locales/ja-JP.json').write_text(json.dumps(data))
        settings.update(self.root,'ja-JP')
        meta,body=read_note(planning.write_plan(self.root))
        self.assertEqual(meta['language'],'ja-JP');self.assertIn('計画テスト',body)

    def test_invalid_locale_and_nonenglish_agent_setting_are_rejected(self):
        with self.assertRaises(ValueError):settings.update(self.root,'../../other')
        path=self.root/settings.SETTINGS_PATH;m,b=read_note(path);m['agent_language']='fr';save_note(path,m,b)
        with self.assertRaisesRegex(ValueError,'fixed to en'):settings.load(self.root)
        self.assertTrue(any(f['code']=='LANGUAGE_SETTINGS' for f in quality.inspect(self.root)))

    def test_source_wrapper_language_changes_but_original_stays_exact(self):
        settings.update(self.root,'en')
        material=Path(self.temp.name)/'source.txt';material.write_text('原始來源不得被改寫。')
        meta,body=read_note(sources.ingest(self.root,material))
        self.assertEqual(meta['language'],'en');self.assertIn('Source material',body)
        self.assertEqual((self.root/meta['asset']).read_bytes(),material.read_bytes())

    def test_language_mismatch_is_visible_without_automatic_translation(self):
        settings.update(self.root,'en')
        self.assertTrue(any(f['code']=='DOCUMENT_LANGUAGE' for f in quality.inspect(self.root)))
        self.assertEqual(read_note(self.root/'04_Requirements/BRD/BRD-001.md')[0]['language'],'zh-TW')

    def test_minimal_okf_unknown_fields_and_broken_links_are_core_conformant(self):
        bundle=Path(self.temp.name)/'minimal';bundle.mkdir()
        (bundle/'concept.md').write_text('---\ntype: Custom Thing\ncustom:\n  nested: true\nverified: {by: process:test, at: "2026-10-09T00:00:00Z"}\n---\n# Concept\n[Future](missing.md)\n')
        self.assertTrue(okf.check_bundle(bundle)['conformant'])

    def test_missing_type_and_duplicate_yaml_keys_fail(self):
        bundle=Path(self.temp.name)/'invalid';bundle.mkdir()
        p=bundle/'concept.md';p.write_text('# No metadata\n')
        self.assertFalse(okf.check_bundle(bundle)['conformant'])
        p.write_text('---\ntype: A\ntype: B\n---\n# Conflicting\n')
        self.assertFalse(okf.check_bundle(bundle)['conformant'])

    def test_reserved_files_follow_their_own_format(self):
        bundle=Path(self.temp.name)/'reserved';bundle.mkdir()
        (bundle/'index.md').write_text('---\nokf_version: "0.2"\n---\n# Items\n- [Future](future.md) - planned\n')
        (bundle/'log.md').write_text('# History\n## 2026-10-09\n- Created the index.\n')
        self.assertTrue(okf.check_bundle(bundle)['conformant'])
        sub=bundle/'sub';sub.mkdir();(sub/'index.md').write_text('---\ntype: Index\n---\n# Invalid\n')
        self.assertFalse(okf.check_bundle(bundle)['conformant'])

    def test_export_preserves_sources_and_skill_runtime_format(self):
        target=Path(self.temp.name)/'bundle'
        native=self.root/'.agents/skills/pm-prd/SKILL.md';before=native.read_bytes()
        result=okf.export(self.root,target)
        self.assertTrue(result['conformant'])
        self.assertEqual(native.read_bytes(),before)
        self.assertNotIn('type',read_note(native)[0])
        meta,body=read_note(target/'90_Agent/Skills/pm-prd.md')
        self.assertEqual(meta['type'],'Agent Skill');self.assertNotIn('[[',body)
        self.assertIn('../../AGENTS.md',body)
        raw='01_Sources/Assets/test-repository/openwiki/quickstart.md'
        self.assertEqual((self.root/raw).read_bytes(),(target/(raw+'.txt')).read_bytes())
        self.assertTrue(okf.check_bundle(target)['conformant'])

    def test_export_refuses_existing_or_nested_destination(self):
        with self.assertRaises(ValueError):okf.export(self.root,self.root/'exports/new')
        target=Path(self.temp.name)/'occupied';target.mkdir()
        with self.assertRaises(ValueError):okf.export(self.root,target)


if __name__=='__main__':unittest.main()
