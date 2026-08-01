#!/usr/bin/env python3
from pathlib import Path
import sys


desktop_path = Path(sys.argv[1])
service_path = Path(sys.argv[2])
app_id = sys.argv[3]
bindir = Path(sys.argv[4])

desktop = desktop_path.read_text(encoding="utf-8").splitlines()
service = service_path.read_text(encoding="utf-8").splitlines()

assert desktop_path.name == f"{app_id}.desktop"
assert service_path.name == f"{app_id}.service"
assert "DBusActivatable=true" in desktop
assert "Exec=sessions-chronicle" in desktop
assert f"Name={app_id}" in service
assert f"Exec={bindir / 'sessions-chronicle'} --gapplication-service" in service
assert all("--sessions-dir" not in line for line in service)
