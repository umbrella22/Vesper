"""Offline regression checks for release probes and CI process-group cleanup."""
from pathlib import Path
import json
import os
import re
import signal
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]


class SourceHealthToolingTests(unittest.TestCase):
    def test_registry_requests_are_bounded_and_preserve_three_states(self):
        source = ROOT / "scripts/plugin-sdk-release.sh"
        with tempfile.TemporaryDirectory() as directory:
            fake = Path(directory) / "curl"
            fake.write_text("""#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
with open(os.environ['CURL_ARGS'], 'a') as out: out.write(json.dumps(args) + '\\n')
mode = os.environ['CURL_MODE']
if mode == 'timeout': sys.exit(28)
if '--write-out' in args: print(mode, end='')
else: print('{"version":{"crate":"fixture","num":"1.0.0","yanked":false,"checksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}')
""")
            fake.chmod(0o755)
            args_path = Path(directory) / "args.jsonl"
            env = dict(os.environ, PATH=directory + os.pathsep + os.environ['PATH'], CURL_ARGS=str(args_path))
            for mode, expected in [("200", 0), ("404", 1), ("timeout", 2)]:
                result = subprocess.run(["bash", "-c", 'source "$1"; set +e; crate_version_status fixture 1.0.0; exit $?', "test", str(source)], env=dict(env, CURL_MODE=mode), capture_output=True, text=True)
                self.assertEqual(result.returncode, expected, result.stderr)
            for mode, expected in [("200", 0), ("timeout", 2)]:
                result = subprocess.run(["bash", "-c", 'source "$1"; set +e; registry_version_checksum fixture 1.0.0; exit $?', "test", str(source)], env=dict(env, CURL_MODE=mode), capture_output=True, text=True)
                self.assertEqual(result.returncode, expected, result.stderr)
            for arguments in map(json.loads, args_path.read_text().splitlines()):
                self.assertIn("--connect-timeout", arguments)
                self.assertIn("--max-time", arguments)
                self.assertGreater(float(arguments[arguments.index("--connect-timeout") + 1]), 0)
                self.assertLessEqual(float(arguments[arguments.index("--max-time") + 1]), 30)

    def test_timeout_kills_term_ignoring_descendant_after_leader_exits(self):
        source = (ROOT / ".github/scripts/run-flutter-ios-subtitle-integration.sh").read_text()
        ruby = re.search(r"<<'RUBY'\n(.*?)\nRUBY", source, re.S).group(1)
        with tempfile.TemporaryDirectory() as directory:
            pidfile = Path(directory) / "child.pid"
            child = Path(directory) / "leader.py"
            child.write_text("""import os, signal, sys, time
null = os.open(os.devnull, os.O_WRONLY)
os.dup2(null, 1)
os.dup2(null, 2)
os.close(null)
signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
pid = os.fork()
if pid == 0:
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    with open(sys.argv[1], 'w') as f: f.write(str(os.getpid()))
    while True: time.sleep(1)
else:
    while True: time.sleep(1)
""")
            pid = None
            try:
                result = subprocess.run(["ruby", "-", "1", "python3", str(child), str(pidfile)], input=ruby, capture_output=True, text=True, timeout=18)
                self.assertEqual(result.returncode, 124, result.stderr)
                pid = int(pidfile.read_text())
                deadline = time.monotonic() + 2
                while self._running(pid) and time.monotonic() < deadline:
                    time.sleep(0.05)
                self.assertFalse(self._running(pid), "TERM-ignoring descendant survived timeout cleanup")
            finally:
                if pid is None and pidfile.exists(): pid = int(pidfile.read_text())
                if pid is not None and self._running(pid):
                    os.kill(pid, signal.SIGKILL)

    @staticmethod
    def _running(pid):
        result = subprocess.run(["ps", "-o", "stat=", "-p", str(pid)], capture_output=True, text=True)
        state = result.stdout.strip()
        return bool(state) and not state.startswith("Z")


if __name__ == "__main__":
    unittest.main()
