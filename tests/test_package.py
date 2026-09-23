"""Regression tests for reproducible package build-environment selection."""

import os
import pathlib
import sys
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from scripts import package  # noqa: E402


class PackageBuildEnvironmentTests(unittest.TestCase):
    @mock.patch.dict(
        os.environ,
        {
            "CARGO_BUILD_TARGET": "aarch64-pc-windows-msvc",
            "CARGO_TARGET_DIR": "C:/poison/target",
            "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER": "wrong-link.exe",
            "CARGO_PROFILE_RELEASE_LTO": "false",
            "CARGO_ENCODED_RUSTFLAGS": "--cfg=poison",
            "RUSTFLAGS": "--cfg=poison",
            "RUSTC_WRAPPER": "wrapper.exe",
            "RUSTC_WORKSPACE_WRAPPER": "workspace-wrapper.exe",
            "RUSTC_BOOTSTRAP": "1",
        },
        clear=False,
    )
    def test_pinned_build_environment_ignores_inherited_cargo_overrides(self):
        toolchain_bin = pathlib.Path("C:/rust/1.97.0/bin")
        environment = package.pinned_build_environment(toolchain_bin, "1.97.0")

        self.assertTrue(environment["PATH"].startswith(str(toolchain_bin)))
        self.assertEqual(environment["RUSTUP_TOOLCHAIN"], "1.97.0")
        self.assertEqual(environment["RUSTC"], str(toolchain_bin / "rustc.exe"))
        self.assertEqual(environment["RUSTDOC"], str(toolchain_bin / "rustdoc.exe"))
        self.assertTrue(
            all(
                name not in environment
                for name in (
                    "CARGO_BUILD_TARGET",
                    "CARGO_TARGET_DIR",
                    "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER",
                    "CARGO_PROFILE_RELEASE_LTO",
                    "CARGO_ENCODED_RUSTFLAGS",
                    "RUSTFLAGS",
                    "RUSTC_WRAPPER",
                    "RUSTC_WORKSPACE_WRAPPER",
                    "RUSTC_BOOTSTRAP",
                )
            )
        )


if __name__ == "__main__":
    unittest.main()
