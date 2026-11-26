"""Write the set W of case 14 (the M4 bar): inputs/<name>.toml and reference/<name>.abi per timed cell, and
tuning/<name>.toml per tuning cell, with the builders and writers of case 11's cells.py.
usage: python -I cells.py <case directory> [seed]   (ASE 3.29.0, spglib 2.8.0, NumPy 2.5.4)
       python -I cells.py --check                   regenerate cases 11 and 14; fail on a file that differs
A seed other than 13 draws other rattles and vacancy sites: held-out variants, which stay out of the repository.

timed cell     atoms  what it stresses                                       tuning cell (rival calibration)
si63_vac       63     8 k-points, one per core; a vacancy                    case 11's input (Si2)
si128          128    Γ only: the most bands, where the subspace work rules  case 11's input (Si2)
al2o3_40       40     two species, rattled; Γ only                           case 11's al2o3
al2o3_slab     30     (0001) slab, Al | O3 … O3 | Al: 12 Å of vacuum; 2×2×1  case 11's al2o3
sto39_vo       39     three species, rattled; an O vacancy in SrTiO3; Γ      tuning/sto5
fe2o3_30       30     AFM Fe2O3 (+ − − + along c): spin, 3d, semicore 3s3p   tuning/fe2o3_10
gaas32_as      32     zincblende GaAs, rattled; an As antisite on a Ga site; Γ  case 11's gaas

A tuning cell holds the element set of its timed cells. It also holds one element that no other tuning cell holds.
The rival calibration (`cargo xtask rivals`) freezes a cutoff per element; that element gives each tuning cell its
own cutoff back.
A cutoff's error is a basis-set error. It barely depends on the k mesh, so fe2o3_10 runs at Γ."""
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
sys.dont_write_bytecode = True  # no __pycache__ in case 11's hashed reference/
SPEC = importlib.util.spec_from_file_location("case11", ROOT / "benchmarks/11_speed_scaling/reference/cells.py")
CASE11 = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CASE11)
VACUUM = 12.0  # Å between the slab's two surfaces; ASE's surface adds half on each side


def primitive(atoms):
    """The primitive cell of `atoms` by spglib, its moments kept: each moment sign is a type of its own."""
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
    """α-Fe2O3 (Blake et al., Am. Mineral. 51, 123 (1966)) in its 30-atom hexagonal cell. The buckled Fe layers
    centred on z = k/6 alternate in sign, so each face-sharing Fe pair is antiparallel: + − − + along c. 4 μB per Fe."""
    atoms = CASE11.corundum_hex(5.038, 13.772, "Fe", 0.3553, 0.3059)
    sign = [1.0 if round(6 * z) % 2 == 0 else -1.0 for z in atoms.get_scaled_positions()[:, 2]]
    atoms.set_initial_magnetic_moments([4.0 * s if e == "Fe" else 0.0
                                        for s, e in zip(sign, atoms.get_chemical_symbols())])
    return atoms


def vacancy(atoms, index):
    return atoms[[i for i in range(len(atoms)) if i != index]]


def cells(seed):
    """(timed, tuning): name → (atoms, mesh). The seed sets each rattle and vacancy site of the timed cells."""
    rng = np.random.Generator(np.random.PCG64(seed))
    si64 = bulk("Si", "diamond", a=CASE11.SI_A, cubic=True).repeat((2, 2, 2))
    si128 = bulk("Si", "diamond", a=CASE11.SI_A, cubic=True).repeat((4, 2, 2))
    sto = crystal(["Sr", "Ti", "O"], [(0, 0, 0), (0.5, 0.5, 0.5), (0.5, 0.5, 0)], spacegroup=221,
                  cellpar=[3.905, 3.905, 3.905, 90, 90, 90])  # SrTiO3 near its measured 3.905 Å
    sto40 = CASE11.ordered(sto.repeat((2, 2, 2)))
    oxygens = [i for i, s in enumerate(sto40.get_chemical_symbols()) if s == "O"]
    species, cell, frac = CASE11.corundum()
    alumina = Atoms(species, scaled_positions=frac, cell=cell, pbc=True)
    slab = surface(CASE11.alumina_hex(), (0, 0, 1), 1, vacuum=VACUUM / 2)
    fe2o3 = hematite()
    gaas = CASE11.ordered(bulk("GaAs", "zincblende", a=5.653, cubic=True).repeat((2, 2, 1)))
    gallium = [i for i, s in enumerate(gaas.get_chemical_symbols()) if s == "Ga"]
    antisite = np.random.Generator(np.random.PCG64(seed + 8))  # its own stream: the draws above stay as they are
    gaas.symbols[gallium[int(antisite.integers(len(gallium)))]] = "As"
    timed = {
        "si63_vac": (CASE11.rattled(vacancy(si64, int(rng.integers(len(si64)))), 0.03, seed + 1), [2, 2, 2]),
        "si128": (CASE11.rattled(si128, 0.05, seed + 2), [1, 1, 1]),
        "al2o3_40": (CASE11.ordered(CASE11.rattled(alumina.repeat((2, 2, 1)), 0.05, seed + 3)), [1, 1, 1]),
        "al2o3_slab": (CASE11.ordered(CASE11.rattled(slab, 0.03, seed + 4)), [2, 2, 1]),
        "sto39_vo": (CASE11.rattled(vacancy(sto40, oxygens[int(rng.integers(len(oxygens)))]), 0.03, seed + 6),
                     [1, 1, 1]),
        "fe2o3_30": (CASE11.ordered(CASE11.rattled(fe2o3, 0.03, seed + 5)), [1, 1, 1]),
        "gaas32_as": (CASE11.ordered(CASE11.rattled(gaas, 0.03, seed + 7)), [1, 1, 1]),
    }
    tuning = {
        "sto5": (CASE11.ordered(sto), [2, 2, 2]),
        "fe2o3_10": (CASE11.ordered(primitive(fe2o3)), [1, 1, 1]),
    }
    return timed, tuning


def replacements(seed):
    """Untimed stand-ins under case.toml's claim-budget rule: name → (timed cell it replaces, atoms, mesh). Each is
    its cell's builder at a smaller size with the same rattle; nothing runs or times them before the claim run."""
    si32 = bulk("Si", "diamond", a=CASE11.SI_A, cubic=True).repeat((2, 2, 1))
    site = np.random.Generator(np.random.PCG64(seed + 9))  # its own stream: the timed cells' draws stay as they are
    return {
        "si31_vac": ("si63_vac", CASE11.rattled(vacancy(si32, int(site.integers(len(si32)))), 0.03, seed + 1),
                     [2, 2, 2]),
        "si96": ("si128", CASE11.rattled(bulk("Si", "diamond", a=CASE11.SI_A, cubic=True).repeat((3, 2, 2)), 0.05,
                                         seed + 2), [1, 1, 1]),
        "al2o3_30": ("al2o3_40", CASE11.ordered(CASE11.rattled(CASE11.alumina_hex(), 0.05, seed + 3)), [1, 1, 1]),
    }


def as_cell(atoms, mesh):
    """`atoms` as case 11's writers take it, its moments None when it has none."""
    moments = atoms.get_initial_magnetic_moments()
    return CASE11.written(*CASE11.from_atoms(atoms), mesh, [float(m) for m in moments] if moments.any() else None)


# ABINIT mixing per cell: it changes the SCF path, not the fixed point. fe2o3_30 under ABINIT's default (diemac 1e6,
# diemix 1) oscillates (80 steps: vres2 ~1e3) and loses its moments; diemac 10 with diemix 0.3 converges to the AFM
# state (Fe ±3.6 μB), vres2 3.9e-11 at step 80, about 1.4 decades per 10 steps (6-variant diagnostic, 4 cores);
# si128 under the metallic default (diemac 1e6) stalls at vres2 ~5e-6 over 20 steps, and diemac 12, Si's dielectric
# constant, is the preconditioner ABINIT's guidance gives for a semiconductor.
MIXING = {"fe2o3_30": ["diemac 10.0", "diemix 0.3"], "si128": ["diemac 12.0"]}
# ABINIT bands per cell, in place of fband 4.0. run.sh gives a Γ cell one rank, and si128's 768 bands at fband 4 took
# ~24 min per step; 320 is ABINIT's default (fband 0.5). SOURCE.toml's empty-top-band check holds it to account.
BANDS = {"si128": 320}
# Every code's SCF step cap on every cell of W: orbitaR's scf.max_iterations, which the rival adapters write as QE's
# electron_maxstep and ABINIT's nstep (xtask harness/rival.rs). SOURCE.toml's reference runs stop at the same cap.
STEPS = 200


def write(case, seed):
    timed, tuning = cells(seed)
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
    for name, (_, atoms, mesh) in replacements(seed).items():
        cell = as_cell(atoms, mesh)
        (case / "replacements" / f"{name}.toml").write_text(CASE11.toml(*cell))
        (case / "replacements" / f"{name}.abi").write_text(CASE11.abi(*cell))
    for path in [p for folder in ("inputs", "tuning", "replacements") for p in (case / folder).glob("*.toml")]:
        text = path.read_text()
        assert text.endswith("\n[scf]\nenergy_tolerance_ha = 1e-9\n"), path
        path.write_text(text + f"max_iterations = {STEPS}\n")


def check():
    """Regenerate cases 11 and 14 into a temporary directory. Name each file that differs, and each committed
    inputs/*.toml, reference/*.abi, tuning/*.toml, replacements/*.toml, or replacements/*.abi that the generator
    no longer writes."""
    runs = {"11_speed_scaling": lambda out: CASE11.write(out, CASE11.CELLS), "14_set_w": lambda out: write(out, 13)}
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
    write(pathlib.Path(sys.argv[1]), int(sys.argv[2]) if len(sys.argv) > 2 else 13)
