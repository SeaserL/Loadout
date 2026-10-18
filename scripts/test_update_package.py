# SPDX-License-Identifier: Apache-2.0
"""Offline installer transactions; no real installation, IPC, registry or processes."""
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
import zipfile

HELPER = Path(__file__).resolve().parents[1]/'packaging/update.ps1'


class PackageTests(unittest.TestCase):
    def run_case(self, extra=None, bad_hash=False, rollback=False):
        with tempfile.TemporaryDirectory(prefix='loadout-update-test-') as temp:
            root=Path(temp); target=root/'install'; target.mkdir()
            names=['loadout.exe','loadoutd.exe','loadout-app.exe']
            for name in names: (target/name).write_bytes(b'old-synthetic')
            package=root/'update.zip'
            with zipfile.ZipFile(package,'w') as archive:
                for name in names: archive.writestr('Loadout-0.4.0-windows-x64/'+name,b'new-synthetic')
                if extra: archive.writestr(extra,b'bad')
            manifest=root/'manifest.json'
            manifest.write_text(json.dumps({'zip':str(package),'sha256':'0'*64 if bad_hash else hashlib.sha256(package.read_bytes()).hexdigest(),'install_dir':str(target),'version':'v0.4.0'}))
            cmd=['powershell','-NoProfile','-ExecutionPolicy','Bypass','-File',str(HELPER),'-Manifest',str(manifest),'-TestRoot',str(root)]
            if rollback: cmd.append('-TestFailAfterCopy')
            result=subprocess.run(cmd,capture_output=True,timeout=40)
            failed=bool(extra or bad_hash or rollback)
            self.assertEqual(result.returncode==0,not failed,result.stderr.decode(errors='replace')[-600:])
            for name in names: self.assertEqual((target/name).read_bytes(), b'old-synthetic' if failed else b'new-synthetic')
            self.assertFalse((root/'escaped').exists())
            if rollback or not failed:
                backups=list(root.glob('transaction-*/backup/loadout.exe'))
                self.assertEqual(len(backups),1); self.assertEqual(backups[0].read_bytes(),b'old-synthetic')

    def test_success_and_backup(self): self.run_case()
    def test_hash_mismatch(self): self.run_case(bad_hash=True)
    def test_traversal(self): self.run_case(extra='Loadout-0.4.0-windows-x64/../escaped')
    def test_unknown_script(self): self.run_case(extra='Loadout-0.4.0-windows-x64/evil.ps1')
    def test_rollback(self): self.run_case(rollback=True)


class BridgeKitTests(unittest.TestCase):
    def check_transaction(self, fail):
        with tempfile.TemporaryDirectory(prefix='loadout-bridge-kit-test-') as temp:
            root=Path(temp); target=root/'existing'; kit=root/'kit'
            target.mkdir(); kit.mkdir()
            names=['opencode_bridge.py','opencode_protocol.py','opencode_events.py','opencode_controls.mjs','Start-OpenCodeBridge.ps1']
            (target/'binding.json').write_text(json.dumps({'python':'synthetic-unused.exe'}))
            for name in names:
                (target/name).write_text('exit 0' if name.endswith('.ps1') else 'old synthetic')
                (kit/name).write_text(('exit 1' if fail else 'exit 0') if name.endswith('.ps1') else 'new synthetic')
            helper=kit/'Update-OpenCodeBridge.ps1'
            helper.write_bytes((HELPER.parents[1]/'scripts/Update-OpenCodeBridge.ps1').read_bytes())
            result=subprocess.run(['powershell','-NoProfile','-ExecutionPolicy','Bypass','-File',str(helper),'-BridgeDir',str(target)],capture_output=True,timeout=20)
            self.assertEqual(result.returncode==0, not fail, result.stderr.decode(errors='replace'))
            self.assertEqual((target/'opencode_bridge.py').read_text(),'old synthetic' if fail else 'new synthetic')
            self.assertEqual(len(list(target.glob('protection-backup-*'))),1)

    def test_bridge_update_only_temporary_scripts(self): self.check_transaction(False)
    def test_bridge_start_failure_restores_scripts(self): self.check_transaction(True)

if __name__=='__main__': unittest.main()
