import argparse
import pathlib
import sys

CASE = pathlib.Path(__file__).resolve().parent
HALF_A_BOHR = 5.168594801950595
PRIMITIVE = ((0.0, HALF_A_BOHR, HALF_A_BOHR), (HALF_A_BOHR, 0.0, HALF_A_BOHR), (HALF_A_BOHR, HALF_A_BOHR, 0.0))
BASIS = ((0.0, 0.0, 0.0), (0.25, 0.25, 0.25))
PRIMITIVE_MESH = (4, 4, 4)
REPEATS = {
    "input.toml": (1, 1, 1),
    "inputs/si8.toml": (2, 2, 1),
    "inputs/si16.toml": (2, 2, 2),
    "inputs/si32.toml": (4, 2, 2),
    "inputs/si64.toml": (4, 4, 2),
    "inputs/si128.toml": (4, 4, 4),
}
SETTINGS = """
[basis]
cutoff_ha = 10.0
density_cutoff_ha = 20.0

[kpoints]
mesh = [{}, {}, {}]
symmetry = false

[occupations]
smearing = "fermi-dirac"
width_ha = 0.00225

[scf]
energy_tolerance_ha = 1e-9
"""


def row(values):
    return "[" + ", ".join(repr(float(v)) for v in values) + "]"


def folded_mesh(repeat):
    mesh = [k // n for k, n in zip(PRIMITIVE_MESH, repeat)]
    assert all(m * n == k for m, n, k in zip(mesh, repeat, PRIMITIVE_MESH)), f"{repeat} does not fold {PRIMITIVE_MESH}"
    return mesh


def supercell(repeat):
    cell = [[n * x for x in vector] for n, vector in zip(repeat, PRIMITIVE)]
    positions = [
        [(i + b[0]) / repeat[0], (j + b[1]) / repeat[1], (k + b[2]) / repeat[2]]
        for i in range(repeat[0])
        for j in range(repeat[1])
        for k in range(repeat[2])
        for b in BASIS
    ]
    return cell, positions


def text(repeat):
    cell, positions = supercell(repeat)
    rows = (",\n" + " " * 13).join(row(r) for r in cell)
    species = ", ".join(['"Si"'] * len(positions))
    sites = ",\n".join("  " + row(p) for p in positions)
    return (
        f"schema = 1\n\n[structure]\ncell_bohr = [{rows}]\nspecies = [{species}]\n"
        f"positions_fractional = [\n{sites}\n]\n" + SETTINGS.format(*folded_mesh(repeat))
    )


def main():
    parser = argparse.ArgumentParser(
        description="Write case 12's diamond Si supercells of case 6's cell, each on the k mesh that folds onto "
        "case 6's 4x4x4."
    )
    parser.add_argument("--check", action="store_true", help="name each file that differs; write nothing")
    check = parser.parse_args().check
    stale = []
    for name, repeat in REPEATS.items():
        path, made = CASE / name, text(repeat)
        if check:
            if not path.is_file() or path.read_text() != made:
                stale.append(name)
        else:
            path.parent.mkdir(exist_ok=True)
            path.write_text(made)
    if check:
        print("\n".join(f"stale: benchmarks/12_memory_scaling/{n}" for n in stale) or "cells: case 12 matches")
    return 1 if stale else 0


if __name__ == "__main__":
    sys.exit(main())
