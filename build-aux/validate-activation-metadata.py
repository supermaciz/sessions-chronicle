#!/usr/bin/env python3
import configparser
from pathlib import Path
import sys


desktop_path = Path(sys.argv[1])
service_path = Path(sys.argv[2])
app_id = sys.argv[3]
bindir = Path(sys.argv[4])
provider_path = Path(sys.argv[5])
provider_service_path = Path(sys.argv[6])

desktop = desktop_path.read_text(encoding="utf-8").splitlines()
service = service_path.read_text(encoding="utf-8").splitlines()


def read_ini(path):
    parser = configparser.ConfigParser(interpolation=None)
    parser.optionxform = str
    parser.read(path, encoding="utf-8")
    return parser


provider = read_ini(provider_path)
provider_service = read_ini(provider_service_path)

provider_bus = f"{app_id}.SearchProvider"
provider_object = f"/{app_id.replace('.', '/')}/SearchProvider"

assert desktop_path.name == f"{app_id}.desktop"
assert service_path.name == f"{app_id}.service"
assert "DBusActivatable=true" in desktop
assert "Exec=sessions-chronicle" in desktop
assert f"Name={app_id}" in service
assert f"Exec={bindir / 'sessions-chronicle'} --gapplication-service" in service
assert all("--sessions-dir" not in line for line in service)

assert provider_path.name == f"{app_id}-search-provider.ini"
assert provider_service_path.name == f"{provider_bus}.service"
assert provider["Shell Search Provider"]["DesktopId"] == desktop_path.name
assert provider["Shell Search Provider"]["BusName"] == provider_bus
assert provider["Shell Search Provider"]["ObjectPath"] == provider_object
assert provider["Shell Search Provider"]["Version"] == "2"
assert provider["Shell Search Provider"]["DefaultDisabled"] == "true"
assert provider_service["D-BUS Service"]["Name"] == provider_bus
assert provider_service["D-BUS Service"]["Exec"] == str(bindir / "sessions-chronicle-search-provider")
assert "--database" not in provider_service["D-BUS Service"]["Exec"]
