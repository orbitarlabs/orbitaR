import pathlib
import sys
import tomllib

from ase.build import bulk, surface

HERE = pathlib.Path(__file__).resolve().parent
HINTS = tomllib.loads((HERE.parents[1] / "datasets/jth-2.0.toml").read_text())["pbe"]["element"]
BOHR_ANGSTROM = 0.529177210903
DECIMALS = 12
MGO_LATTICE_ANGSTROM_EXPERIMENT = 4.212
CUBIC_001_IN_PRIMITIVE_FCC_MILLER = (1, 1, 0)
MGO_PLANES = 6
VACUUM_EACH_SIDE_ANGSTROM = 7.5
MESH = [4, 4, 1]
OUTPUT = HERE / "mgo001_slab.toml"
USAGE = "usage: python -I slab.py [--check]"


def rows(values):
    return ",\n".join("  [" + ", ".join(repr(round(float(x), DECIMALS) + 0.0) for x in row) + "]" for row in values)


def text():
    slab = surface(bulk("MgO", "rocksalt", a=MGO_LATTICE_ANGSTROM_EXPERIMENT), CUBIC_001_IN_PRIMITIVE_FCC_MILLER,
                   layers=MGO_PLANES, vacuum=VACUUM_EACH_SIDE_ANGSTROM)
    species = slab.get_chemical_symbols()
    cutoff = max(HINTS[s]["cutoff_hint_ha"] for s in set(species))
    lines = [
        "schema = 1", "", "[structure]", f"cell_bohr = [\n{rows(slab.cell[:] / BOHR_ANGSTROM)}\n]",
        "species = [" + ", ".join(f'"{s}"' for s in species) + "]",
        f"positions_fractional = [\n{rows(slab.get_scaled_positions(wrap=False))}\n]",
        "", "[basis]", f"cutoff_ha = {cutoff}", f"density_cutoff_ha = {2 * cutoff}",
        "", "[kpoints]", f"mesh = {MESH}", "symmetry = false",
        "", "[occupations]", 'smearing = "fermi-dirac"', "width_ha = 0.00225",
        "", "[scf]", "energy_tolerance_ha = 1e-9",
    ]
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    match sys.argv[1:]:
        case []:
            OUTPUT.write_text(text())
        case ["--check"]:
            if OUTPUT.read_text() != text():
                sys.exit(f"{OUTPUT.name} differs from what slab.py writes")
        case _:
            sys.exit(USAGE)
