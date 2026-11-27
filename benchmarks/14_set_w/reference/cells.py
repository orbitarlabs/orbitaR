import filecmp
import importlib.util
import pathlib
import sys
import tempfile

import numpy as np
import spglib
from ase import Atoms
from ase.build import bulk, surface
from ase.spacegroup import crystal

ROOT = pathlib.Path(__file__).resolve().parents[3]
sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location("case11", ROOT / "benchmarks/11_speed_scaling/reference/cells.py")
CASE11 = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CASE11)
VACUUM_BETWEEN_SURFACES_ANGSTROM = 12.0
SRTIO3_LATTICE_ANGSTROM = 3.905
COMMITTED_SEED = 13
USAGE = """usage: python -I cells.py <case directory> [seed]   seed 13 writes the committed cells; another seed draws
                                                    held-out variants, which stay out of the repository
       python -I cells.py --check                   regenerate cases 11 and 14; fail on a file that differs"""


def primitive_keeping_moments(atoms):
    moments = atoms.get_initial_magnetic_moments()
    types = [z + 1000 * (m < 0) + 2000 * (m > 0) for z, m in zip(atoms.numbers, moments)]
    cell, frac, numbers = spglib.standardize_cell((atoms.cell[:], atoms.get_scaled_positions(), types),
                                                  to_primitive=True, no_idealize=True)
    size = {t: abs(m) for t, m in zip(types, moments)}
    assert all(abs(abs(m) - size[t]) < 1e-12 for t, m in zip(types, moments)), "one moment size per type"
    reduced = Atoms(numbers=[n % 1000 for n in numbers], scaled_positions=frac % 1.0, cell=cell, pbc=True)
    reduced.set_initial_magnetic_moments([size[n] * (-1 if n // 1000 == 1 else 1) for n in numbers])
    return reduced


def hematite():
    atoms = CASE11.corundum_hexagonal(5.038, 13.772, "Fe", 0.3553, 0.3059)
    sign = [1.0 if round(6 * z) % 2 == 0 else -1.0 for z in atoms.get_scaled_positions()[:, 2]]
    atoms.set_initial_magnetic_moments([4.0 * s if e == "Fe" else 0.0
                                        for s, e in zip(sign, atoms.get_chemical_symbols())])
    return atoms


def vacancy(atoms, index):
    return atoms[[i for i in range(len(atoms)) if i != index]]


def timed_and_tuning_cells(seed):
    rng = np.random.Generator(np.random.PCG64(seed))
    si64 = bulk("Si", "diamond", a=CASE11.SI_LATTICE_ANGSTROM, cubic=True).repeat((2, 2, 2))
    si128 = bulk("Si", "diamond", a=CASE11.SI_LATTICE_ANGSTROM, cubic=True).repeat((4, 2, 2))
    sto = crystal(["Sr", "Ti", "O"], [(0, 0, 0), (0.5, 0.5, 0.5), (0.5, 0.5, 0)], spacegroup=221,
                  cellpar=[SRTIO3_LATTICE_ANGSTROM] * 3 + [90, 90, 90])
    sto40 = CASE11.one_block_per_element(sto.repeat((2, 2, 2)))
    oxygens = [i for i, s in enumerate(sto40.get_chemical_symbols()) if s == "O"]
    species, cell, frac = CASE11.alumina_rhombohedral()
    alumina = Atoms(species, scaled_positions=frac, cell=cell, pbc=True)
    slab = surface(CASE11.alumina_hexagonal(), (0, 0, 1), 1, vacuum=VACUUM_BETWEEN_SURFACES_ANGSTROM / 2)
    fe2o3 = hematite()
    gaas = CASE11.one_block_per_element(bulk("GaAs", "zincblende", a=5.653, cubic=True).repeat((2, 2, 1)))
    gallium = [i for i, s in enumerate(gaas.get_chemical_symbols()) if s == "Ga"]
    antisite_stream = np.random.Generator(np.random.PCG64(seed + 8))
    gaas.symbols[gallium[int(antisite_stream.integers(len(gallium)))]] = "As"
    timed = {
        "si63_vac": (CASE11.rattled(vacancy(si64, int(rng.integers(len(si64)))), 0.03, seed + 1), [2, 2, 2]),
        "si128": (CASE11.rattled(si128, 0.05, seed + 2), [1, 1, 1]),
        "al2o3_40": (CASE11.one_block_per_element(CASE11.rattled(alumina.repeat((2, 2, 1)), 0.05, seed + 3)),
                     [1, 1, 1]),
        "al2o3_slab": (CASE11.one_block_per_element(CASE11.rattled(slab, 0.03, seed + 4)), [2, 2, 1]),
        "sto39_vo": (CASE11.rattled(vacancy(sto40, oxygens[int(rng.integers(len(oxygens)))]), 0.03, seed + 6),
                     [1, 1, 1]),
        "fe2o3_30": (CASE11.one_block_per_element(CASE11.rattled(fe2o3, 0.03, seed + 5)), [1, 1, 1]),
        "gaas32_as": (CASE11.one_block_per_element(CASE11.rattled(gaas, 0.03, seed + 7)), [1, 1, 1]),
    }
    tuning = {
        "sto5": (CASE11.one_block_per_element(sto), [2, 2, 2]),
        "fe2o3_10": (CASE11.one_block_per_element(primitive_keeping_moments(fe2o3)), [1, 1, 1]),
    }
    return timed, tuning


def untimed_replacements(seed):
    si32 = bulk("Si", "diamond", a=CASE11.SI_LATTICE_ANGSTROM, cubic=True).repeat((2, 2, 1))
    si96 = bulk("Si", "diamond", a=CASE11.SI_LATTICE_ANGSTROM, cubic=True).repeat((3, 2, 2))
    alumina30 = CASE11.alumina_hexagonal()
    vacancy_stream = np.random.Generator(np.random.PCG64(seed + 9))
    return {
        "si31_vac": ("si63_vac", CASE11.rattled(vacancy(si32, int(vacancy_stream.integers(len(si32)))), 0.03,
                                                seed + 1), [2, 2, 2]),
        "si96": ("si128", CASE11.rattled(si96, 0.05, seed + 2), [1, 1, 1]),
        "al2o3_30": ("al2o3_40", CASE11.one_block_per_element(CASE11.rattled(alumina30, 0.05, seed + 3)), [1, 1, 1]),
    }


def as_cell(atoms, mesh):
    moments = atoms.get_initial_magnetic_moments()
    return CASE11.written(*CASE11.from_atoms(atoms), mesh, [float(m) for m in moments] if moments.any() else None)


MIXING = {"fe2o3_30": ["diemac 10.0", "diemix 0.3"], "si128": ["diemac 12.0"]}
BANDS = {"si128": 320}
STEPS = 200


def write(case, seed):
    timed, tuning = timed_and_tuning_cells(seed)
    CASE11.write(case, {n: as_cell(*c) for n, c in timed.items()}, {n: as_cell(*c) for n, c in tuning.items()})
    for name, lines in MIXING.items():
        path = case / "reference" / f"{name}.abi"
        path.write_text(path.read_text() + "\n".join(lines) + "\n")
    for name, count in BANDS.items():
        path = case / "reference" / f"{name}.abi"
        text = path.read_text()
        assert text.count("\nfband 4.0\n") == 1
        path.write_text(text.replace("\nfband 4.0\n", f"\nnband {count}\n"))
    (case / "replacements").mkdir(exist_ok=True)
    for name, (_, atoms, mesh) in untimed_replacements(seed).items():
        cell = as_cell(atoms, mesh)
        (case / "replacements" / f"{name}.toml").write_text(CASE11.toml(*cell))
        (case / "replacements" / f"{name}.abi").write_text(CASE11.abi(*cell))
    for path in [p for folder in ("inputs", "tuning", "replacements") for p in (case / folder).glob("*.toml")]:
        text = path.read_text()
        assert text.endswith("\n[scf]\nenergy_tolerance_ha = 1e-9\n"), path
        path.write_text(text + f"max_iterations = {STEPS}\n")


def check():
    runs = {"11_speed_scaling": lambda out: CASE11.write(out, CASE11.CELLS),
            "14_set_w": lambda out: write(out, COMMITTED_SEED)}
    stale = []
    with tempfile.TemporaryDirectory() as tmp:
        for case, run in runs.items():
            out = pathlib.Path(tmp) / case
            run(out)
            made = {p.relative_to(out) for p in out.rglob("*") if p.is_file()}
            kept = {p.relative_to(ROOT / "benchmarks" / case)
                    for pattern in ("inputs/*.toml", "reference/*.abi", "tuning/*.toml", "replacements/*.toml",
                                    "replacements/*.abi")
                    for p in (ROOT / "benchmarks" / case).glob(pattern)}
            for rel in sorted(made | kept):
                old, new = ROOT / "benchmarks" / case / rel, out / rel
                if not (rel in made and old.is_file() and filecmp.cmp(new, old, shallow=False)):
                    stale.append(f"benchmarks/{case}/{rel}")
    print("\n".join(f"stale: {path}" for path in stale) or "cells: cases 11 and 14 match their generators")
    return 1 if stale else 0


if __name__ == "__main__":
    if sys.argv[1:] == ["--check"]:
        sys.exit(check())
    if not 2 <= len(sys.argv) <= 3:
        sys.exit(USAGE)
    write(pathlib.Path(sys.argv[1]), int(sys.argv[2]) if len(sys.argv) > 2 else COMMITTED_SEED)
