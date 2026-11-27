import argparse
import shutil
import tomllib
from pathlib import Path

from orbitar import datasets

PACKAGE = Path(__file__).resolve().parent
GROUP = "orbitar.datasets"


def family_and_module(package=PACKAGE):
    project = tomllib.loads((package / "pyproject.toml").read_text())
    [(family, module)] = project["project"]["entry-points"][GROUP].items()
    return family, module


def stage(fetched, package=PACKAGE, orbitar="orbitar"):
    family, module = family_and_module(package)
    fetched = Path(fetched)
    fetched.mkdir(parents=True)
    datasets.fetch(fetched, family=family, orbitar=orbitar)
    target = Path(package, module)
    for entry in sorted(fetched.iterdir()):
        if entry.suffix == ".toml" and entry.name != f"{family}.toml":
            continue
        if entry.is_dir():
            shutil.copytree(entry, target / entry.name, copy_function=shutil.copy)
        else:
            shutil.copy(entry, target / entry.name)


def main(argv=None):
    parser = argparse.ArgumentParser(
        prog="python -I packages/orbitar-jth/stage.py",
        description="Fetch the data package's family into a new directory and stage its files and its table into "
        "the package (container/Dockerfile jth-build, `cargo xtask wheel --target`).",
    )
    parser.add_argument("fetched", help="a directory that does not exist yet")
    parser.add_argument("--orbitar", default="orbitar", help="the orbitar binary that names the files")
    args = parser.parse_args(argv)
    stage(args.fetched, orbitar=args.orbitar)


if __name__ == "__main__":
    main()
