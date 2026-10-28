import contextlib
import io
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import bump_versions as bump


def make_workspace(root):
    for name in bump.MANIFESTS:
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text('[package]\nversion = "0.1.0"\n')
    (root / "meson.build").write_text("project('app',\n  version: '0.1.0'\n)\n")
    (root / "Cargo.lock").write_text("original lockfile\n")


class BumpTests(unittest.TestCase):
    def test_success_updates_every_version(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_workspace(root)
            with patch.object(bump, "find_root", return_value=root), patch.object(
                bump.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)
            ) as run, patch("sys.argv", ["bump", "0.2.0"]), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(bump.main(), 0)
            run.assert_called_once_with(["cargo", "update", "--workspace", "--offline"], cwd=root, check=True)
            for name in bump.MANIFESTS:
                self.assertEqual((root / name).read_text(), '[package]\nversion = "0.2.0"\n')
            self.assertEqual((root / "meson.build").read_text(), "project('app',\n  version: '0.2.0'\n)\n")

    def test_interrupt_restores_files_and_exits_130(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_workspace(root)
            originals = {p: p.read_bytes() for p in root.rglob("*") if p.is_file()}
            with patch.object(bump, "find_root", return_value=root), patch.object(
                bump.subprocess, "run", side_effect=KeyboardInterrupt
            ), patch("sys.argv", ["bump", "0.2.0"]), contextlib.redirect_stdout(
                io.StringIO()
            ), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(bump.main(), 130)
            self.assertEqual({p: p.read_bytes() for p in originals}, originals)


class RollbackTests(unittest.TestCase):
    def test_failures_restore_original_files(self):
        for failure in ("missing_cargo", "write", "cargo_exit"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                make_workspace(root)
                originals = {p: p.read_bytes() for p in root.rglob("*") if p.is_file()}
                write_text = Path.write_text
                failed = False

                def write(path, content, **kwargs):
                    nonlocal failed
                    if failure == "write" and path == root / bump.MANIFESTS[1] and not failed:
                        failed = True
                        write_text(path, "partial write")
                        raise OSError("write failed")
                    return write_text(path, content, **kwargs)

                def cargo(*args, **kwargs):
                    if failure == "missing_cargo":
                        raise FileNotFoundError("cargo missing")
                    (root / "Cargo.lock").write_text("changed lockfile")
                    raise subprocess.CalledProcessError(1, "cargo")

                with patch.object(bump, "find_root", return_value=root), patch.object(
                    bump.subprocess, "run", side_effect=cargo
                ), patch.object(Path, "write_text", write), patch(
                    "sys.argv", ["bump", "0.2.0"]
                ), contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(bump.main(), 1)
                self.assertEqual({p: p.read_bytes() for p in originals}, originals)


if __name__ == "__main__":
    unittest.main()
