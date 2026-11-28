import pathlib
import sys
import tomllib

from ase.build import bulk, fcc111, surface

HERE = pathlib.Path(__file__).resolve().parent
HINTS = tomllib.loads((HERE.parents[1] / "datasets/jth-2.0.toml").read_text())["pbe"]["element"]
BOHR_ANGSTROM = 0.529177210903
DECIMALS = 12
VACUUM_EACH_SIDE_ANGSTROM = 7.5
MGO_LATTICE_ANGSTROM_EXPERIMENT = 4.212
CUBIC_001_IN_PRIMITIVE_FCC_MILLER = (1, 1, 0)
MGO_PLANES = 6
MGO_MESH = [4, 4, 1]
AL_LATTICE_ANGSTROM_EXPERIMENT = 4.0495
AL_PLANES = 9
AL_MESH = [9, 9, 1]
MAX_ITERATIONS = 300
USAGE = "usage: python -I slab.py [--check]"


def mgo001():
    return surface(bulk("MgO", "rocksalt", a=MGO_LATTICE_ANGSTROM_EXPERIMENT), CUBIC_001_IN_PRIMITIVE_FCC_MILLER,
                   layers=MGO_PLANES, vacuum=VACUUM_EACH_SIDE_ANGSTROM), MGO_MESH


def al111():
    return fcc111("Al", size=(1, 1, AL_PLANES), a=AL_LATTICE_ANGSTROM_EXPERIMENT,
                  vacuum=VACUUM_EACH_SIDE_ANGSTROM), AL_MESH


CELLS = {"mgo001_slab.toml": mgo001, "al111_slab.toml": al111}


def rows(values):
    return ",\n".join("  [" + ", ".join(repr(round(float(x), DECIMALS) + 0.0) for x in row) + "]" for row in values)


def text(slab, mesh):
    species = slab.get_chemical_symbols()
    cutoff = max(HINTS[s]["cutoff_hint_ha"] for s in set(species))
    lines = [
        "schema = 1", "", "[structure]", f"cell_bohr = [\n{rows(slab.cell[:] / BOHR_ANGSTROM)}\n]",
        "species = [" + ", ".join(f'"{s}"' for s in species) + "]",
        f"positions_fractional = [\n{rows(slab.get_scaled_positions(wrap=False))}\n]",
        "", "[basis]", f"cutoff_ha = {cutoff}", f"density_cutoff_ha = {2 * cutoff}",
        "", "[kpoints]", f"mesh = {mesh}", "symmetry = false",
        "", "[occupations]", 'smearing = "fermi-dirac"', "width_ha = 0.00225",
        "", "[scf]", "energy_tolerance_ha = 1e-9", f"max_iterations = {MAX_ITERATIONS}",
    ]
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    match sys.argv[1:]:
        case []:
            for name, cell in CELLS.items():
                (HERE / name).write_text(text(*cell()))
        case ["--check"]:
            for name, cell in CELLS.items():
                if (HERE / name).read_bytes() != text(*cell()).encode():
                    sys.exit(f"{name} differs from what slab.py writes")
        case _:
            sys.exit(USAGE)
