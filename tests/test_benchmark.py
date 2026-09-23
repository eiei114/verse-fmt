"""Regression tests for optional benchmark probes and stderr capture."""

import pathlib
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from scripts import benchmark  # noqa: E402


class BenchmarkTests(unittest.TestCase):
    def test_run_sample_drains_large_stderr_and_keeps_failure_prefix(self):
        command = [
            sys.executable,
            "-c",
            "import sys; sys.stderr.write('x' * (2 * 1024 * 1024)); sys.exit(7)",
        ]
        with tempfile.TemporaryDirectory() as directory:
            sample = benchmark.run_sample(command, pathlib.Path(directory))

        self.assertEqual(sample["exit_code"], 7)
        self.assertEqual(sample["stderr"], "x" * 500)

    def test_git_metadata_is_optional_when_git_is_missing(self):
        with mock.patch.object(benchmark.subprocess, "run", side_effect=FileNotFoundError):
            self.assertIsNone(benchmark.git_head(ROOT))

    def test_optional_run_returns_none_when_executable_is_missing(self):
        with mock.patch.object(benchmark.subprocess, "run", side_effect=FileNotFoundError):
            self.assertIsNone(benchmark.optional_run(["not-installed-tool"]))


if __name__ == "__main__":
    unittest.main()
