import json
import sys
from pathlib import Path

USAGE = "usage: python3 -I reduce.py [--abo <run.abo>] <reference.json> <bands> <Z>:<count> ... > <name>.json"
TERMS = ["kinetic_energy_ha", "hartree_energy_ha", "xc_energy_ha", "ewald_energy_ha", "psp_core_energy_ha",
         "local_energy_ha", "one_center_energy_ha", "smearing_energy_ha"]


def left_to_right_sum(values):
    total = 0.0
    for value in values:
        total += value
    return total


def pseudo_minus_all_electron_core_tau_ha():
    table = json.loads((Path(__file__).parent / "exccore.json").read_text())
    pseudo, ae = table["pseudo_core_tau_ha"], table["all_electron_core_tau_ha"]
    return {int(z): pseudo[z] - ae[z] for z in pseudo}


def last_cartesian_forces_and_removed_mean(abo):
    lines = Path(abo).read_text().splitlines()
    start = max(i for i, line in enumerate(lines) if line.strip() == "cartesian forces (hartree/bohr) at end:")
    rows, i = [], start + 1
    while not lines[i].strip().startswith("frms,max,avg="):
        rows.append([float(x) for x in lines[i].split()[1:4]])
        i += 1
    mean = [float(x) for x in lines[i].split("=")[1].split()[2:5]]
    return rows, mean


args = sys.argv[1:]
if len(args) < (5 if args[:1] == ["--abo"] else 3):
    sys.exit(USAGE)
abo = None
if args[:1] == ["--abo"]:
    abo, args = args[1], args[2:]
source = json.loads(Path(args[0]).read_text())
bands = int(args[1])
counts = [tuple(int(x) for x in a.split(":")) for a in args[2:]]
delta = pseudo_minus_all_electron_core_tau_ha()
out = {key: source[key] for key in TERMS}
out["free_energy_ha"] = left_to_right_sum(source[key] for key in TERMS)
out["core_offset_ha"] = left_to_right_sum(delta[z] * n for z, n in counts)
out["fermi_level_ha"] = source["fermi_level_ha"]
out["kpoints_fractional"] = source["kpoints_fractional"]
out["eigenvalues_ha"] = [[k[:bands] for k in spin] for spin in source["eigenvalues_ha"]]
if "magnetization_bohr_magneton" in source:
    out["magnetization_bohr_magneton"] = source["magnetization_bohr_magneton"]
if abo is not None:
    out["forces_ha_bohr"], out["force_mean_ha_bohr"] = last_cartesian_forces_and_removed_mean(abo)
print(json.dumps(out, indent=1))
