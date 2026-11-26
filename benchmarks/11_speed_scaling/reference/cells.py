"""Write the cells of case 11: inputs/<name>.toml for orbitaR and reference/<name>.abi for ABINIT, from one cell.
Both files carry one cell in Bohr, the cutoffs, and the mesh. orbitaR reads reduced positions; ABINIT reads Cartesian
ones in Bohr (xcart), 12 decimals of the same product: no reference file then holds a fraction such as 2/3 that the
science scan S1 would match to an engine literal.
usage: python -I cells.py <case directory>        (ASE 3.29.0, spglib 2.8.0, NumPy 2.5.4)
Case 14 uses this file's builders and writers: run `python -I benchmarks/14_set_w/reference/cells.py --check` after
a change here."""
import pathlib, sys, tomllib
import numpy as np
import spglib
from ase.build import bulk, make_supercell
from ase.data import atomic_numbers, chemical_symbols
from ase.spacegroup import crystal

BOHR = 0.529177210903  # Å, CODATA 2018
SI_A = 5.431  # Å, diamond Si near its measured lattice constant
HINTS = tomllib.loads((pathlib.Path(__file__).resolve().parents[3] / "datasets/jth-2.0.toml").read_text())["pbe"][
    "element"]


def cutoff(species):
    """A cell's cutoff in Ha: the highest JTH PBE hint of its elements."""
    return max(HINTS[s]["cutoff_hint_ha"] for s in set(species))


def ordered(atoms):
    """Species in a stable order: one block per element."""
    return atoms[np.argsort(atoms.get_chemical_symbols(), kind="stable")]


def corundum_hex(a, c, metal, z_metal, x_oxygen):
    """M2O3 in R-3c, the 30-atom hexagonal cell: M at 12c (0, 0, z), O at 18e (x, 0, 1/4)."""
    return crystal([metal, "O"], [(0, 0, z_metal), (x_oxygen, 0, 0.25)], spacegroup=167,
                   cellpar=[a, a, c, 90, 90, 120])


def alumina_hex():
    """α-Al2O3 (Kirfel and Eichhorn, Acta Cryst. A46, 271 (1990))."""
    return corundum_hex(4.7602, 12.9933, "Al", 0.35216, 0.30624)


def rattled(atoms, amplitude, seed):
    """Moves each atom by up to `amplitude` Å along each axis: PCG64 (NumPy's documented stable stream)."""
    atoms = atoms.copy()
    atoms.positions += np.random.Generator(np.random.PCG64(seed)).uniform(-amplitude, amplitude, (len(atoms), 3))
    return atoms


def corundum():
    """α-Al2O3 in its 10-atom rhombohedral cell."""
    hexagonal = alumina_hex()
    cell, frac, numbers = spglib.standardize_cell(
        (hexagonal.cell[:], hexagonal.get_scaled_positions(), hexagonal.numbers), to_primitive=True, no_idealize=True)
    order = sorted(range(len(numbers)), key=lambda i: (-numbers[i], tuple(frac[i] % 1.0)))
    return [chemical_symbols[numbers[i]] for i in order], cell, np.array([frac[i] % 1.0 for i in order])


def nio():
    """Rocksalt NiO in its AFM-II cell: ferromagnetic (111) Ni planes, alternate in sign along [111]; 2 μB per Ni."""
    atoms = make_supercell(bulk("NiO", "rocksalt", a=4.17), [[0, 1, 1], [1, 0, 1], [1, 1, 0]])
    species, cell, frac = from_atoms(ordered(atoms))
    plane = [round(float(np.sum(f)) * 2) % 2 for f in frac]
    return species, cell, frac, [(2.0 if p == 0 else -2.0) if s == "Ni" else 0.0 for s, p in zip(species, plane)]


def from_atoms(atoms):
    return atoms.get_chemical_symbols(), atoms.cell[:], atoms.get_scaled_positions(wrap=False)


def written(species, cell, frac, mesh, moments=None):
    """A cell as the writers take it: (species, cell Å, reduced positions, cutoff Ha, mesh, start moments μB)."""
    return species, cell, frac, cutoff(species), mesh, moments


# The lattice constants are near the measured ones; the comparison with ABINIT does not depend on them.
*afm, moments = nio()
wurtzite = bulk("GaAs", "wurtzite", a=3.989, c=6.564).repeat((1, 1, 2))
p1 = rattled(bulk("Si", "diamond", a=SI_A, cubic=True).repeat((2, 2, 1)), 0.05, 11)
CELLS = {
    "gaas": written(*from_atoms(wurtzite), [2, 2, 1]),
    "al2o3": written(*corundum(), [2, 1, 1]),
    "si32": written(*from_atoms(p1), [1, 1, 2]),
    "nio": written(*afm, [2, 1, 1], moments),
}


def clean(values):
    """12 decimals, and no negative zero: the files do not carry the last bits of a matrix product."""
    return np.round(np.asarray(values, dtype=float), 12) + 0.0


def rows(values):
    return ",\n".join("  [" + ", ".join(repr(float(x)) for x in clean(row)) + "]" for row in values)


def toml(species, cell, frac, cutoff, mesh, moment):
    text = [
        "schema = 1", "", "[structure]", f"cell_bohr = [\n{rows(cell / BOHR)}\n]",
        "species = [" + ", ".join(f'"{s}"' for s in species) + "]", f"positions_fractional = [\n{rows(frac)}\n]",
        "", "[basis]", f"cutoff_ha = {cutoff}", f"density_cutoff_ha = {2 * cutoff}",
        "", "[kpoints]", f"mesh = {mesh}", "symmetry = false",
    ]
    if moment is not None:
        text += ["", "[spin]", "moments = [" + ", ".join(map(str, moment)) + "]"]
    text += ["", "[occupations]", 'smearing = "fermi-dirac"', "width_ha = 0.00225",
             "", "[scf]", "energy_tolerance_ha = 1e-9"]
    return "\n".join(text) + "\n"


def abi(species, cell, frac, cutoff, mesh, moment):
    kinds = sorted(set(species), key=species.index)
    text = [
        "acell 3*1.0", "rprim\n" + "\n".join(" ".join(repr(float(x)) for x in clean(v)) for v in cell / BOHR),
        f"natom {len(species)}", f"ntypat {len(kinds)}", "znucl " + " ".join(str(atomic_numbers[k]) for k in kinds),
        "typat " + " ".join(str(kinds.index(s) + 1) for s in species),
        "xcart\n" + "\n".join(" ".join(repr(float(x)) for x in clean(p)) for p in frac @ (cell / BOHR)),
        'pseudos "' + ", ".join(f"{k}/{k}.GGA_PBE-JTH.xml" for k in kinds) + '"',
        f"ecut {cutoff}", f"pawecutdg {2 * cutoff}", "ngkpt " + " ".join(map(str, mesh)), "nshiftk 1", "shiftk 0 0 0",
        "occopt 3", "tsmear 0.00225", "fband 4.0", f"istwfk {int(np.prod(mesh))}*1",
    ]
    if moment is not None:
        text += ["nsppol 2", "spinat\n" + "\n".join(f"0 0 {m}" for m in moment)]
    return "\n".join(text) + "\n"


def write(case, cells, tuning=None):
    """inputs/<name>.toml and reference/<name>.abi per cell; tuning/<name>.toml per tuning cell."""
    for folder in ("inputs", "reference") + (("tuning",) if tuning else ()):
        (case / folder).mkdir(parents=True, exist_ok=True)
    for name, cell in cells.items():
        (case / "inputs" / f"{name}.toml").write_text(toml(*cell))
        (case / "reference" / f"{name}.abi").write_text(abi(*cell))
    for name, cell in (tuning or {}).items():
        (case / "tuning" / f"{name}.toml").write_text(toml(*cell))


if __name__ == "__main__":
    write(pathlib.Path(sys.argv[1]), CELLS)
