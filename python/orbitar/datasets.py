import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import urllib.request
from pathlib import Path, PurePosixPath

TABLES = Path(__file__).with_name("families")
UMASK = os.umask(0o022)
os.umask(UMASK)
NOTICE = (
    "This command downloads each pinned file that `orbitar datasets --directory <directory>{family}` names, from its "
    "source; each file keeps its source's license.\n"
    "The data package orbitar-jth distributes the packaged jth-2.0 family separately (GPL-3.0)."
)


def tables(directory, out):
    Path(directory).mkdir(parents=True, exist_ok=True)
    for table in sorted(TABLES.glob("*.toml")):
        target = Path(directory, table.name)
        if not (target.is_file() and target.read_bytes() == table.read_bytes()):
            target.write_bytes(table.read_bytes())
            print(f"wrote    {table.name}", file=out)


def rows(directory, family, orbitar=("orbitar",)):
    words = [*orbitar, "datasets", "--directory", str(directory)] + (["--family", family] if family else [])
    out = subprocess.run(words, capture_output=True, text=True)
    if out.returncode != 0:
        raise ValueError(f"orbitar.datasets: {out.stderr.strip()}")
    return json.loads(out.stdout)


def chosen(found, elements):
    if not elements:
        return found
    picked = [row for row in found if row["element"] in elements]
    missing = sorted(set(elements) - {row["element"] for row in picked})
    if missing:
        raise SystemExit(f"orbitar.datasets: the family holds no file for {missing}; `orbitar datasets` lists them")
    return picked


def fetch(directory, elements=(), family=None, orbitar="orbitar", out=None):
    out = out or sys.stdout
    print(NOTICE.format(family=f" --family {family}" if family else ""), file=out)
    tables(directory, out)
    try:
        found = rows(directory, family, [orbitar])
    except ValueError as refusal:
        raise SystemExit(str(refusal)) from None
    for row in chosen(found, elements):
        path, sha = row["path"], row["sha256"]
        relative = PurePosixPath(path)
        if relative.is_absolute() or ".." in relative.parts:
            raise SystemExit(f"orbitar.datasets: {path} leaves the directory")
        target = Path(directory, *relative.parts)
        if target.is_file() and hashlib.sha256(target.read_bytes()).hexdigest() == sha:
            print(f"kept     {path}", file=out)
            continue
        if "url" not in row:
            raise SystemExit(f"orbitar.datasets: {path} has no source; build it from its input in the repository")
        with urllib.request.urlopen(row["url"], timeout=60) as response:
            data = response.read()
        got = hashlib.sha256(data).hexdigest()
        if got != sha:
            raise SystemExit(f"orbitar.datasets: {row['url']} has sha256 {got}, but the pin is {sha}; nothing written")
        target.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(dir=target.parent, delete=False) as temporary_download:
            temporary_download.write(data)
        os.chmod(temporary_download.name, 0o666 & ~UMASK)
        os.replace(temporary_download.name, target)
        print(f"fetched  {path}", file=out)


def main(argv=None):
    parser = argparse.ArgumentParser(prog="python -I -m orbitar.datasets")
    commands = parser.add_subparsers(dest="command", required=True)
    command = commands.add_parser("fetch", help="download the files of a family, each checked by its sha256")
    command.add_argument("--family", help="a family that `orbitar datasets` knows; default: the engine's default")
    command.add_argument("directory", help="the [datasets] directory of the inputs, e.g. datasets")
    command.add_argument("elements", nargs="*", help="element symbols, e.g. Si O; every file when none")
    command.add_argument("--orbitar", default="orbitar", help=argparse.SUPPRESS)
    args = parser.parse_args(argv)
    fetch(args.directory, args.elements, args.family, args.orbitar)


if __name__ == "__main__":
    main()
