"""Render absolute paths and run an isolated, loopback-only ClickHouse server."""

import argparse
import os
from pathlib import Path

p = argparse.ArgumentParser()
p.add_argument("--binary", type=Path, required=True)
p.add_argument("--work-dir", type=Path, required=True)
args = p.parse_args()
root = args.work_dir.resolve()
for name in ["data", "tmp", "log"]:
    (root / "ch" / name).mkdir(parents=True, exist_ok=True)
configuration = (
    (Path(__file__).parent / "server.xml")
    .read_text()
    .replace("./ch/", str(root / "ch") + "/")
)
config_path = root / "server.xml"
config_path.write_text(configuration)
binary = args.binary.resolve()
os.chdir(root)
os.execv(str(binary), [str(binary), "server", f"--config-file={config_path}"])
