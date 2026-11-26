"""One reference/<name>.json of case 20 from the reference.json that case 6's reference.py wrote for ABINIT's run of
<name>.abi: the terms as written; free_energy_ha the direct form, the sum of those terms (ABINIT's etotal is the
double-counting one, which iscf 17 prints); the first `bands` eigenvalues of each k-point (each .abi computes 4 more,
nbdbuf 4, so that tolwfr holds on these); and core_offset_ha = Σ N_s δ_s from exccore.json, δ_s = E_xc[n_c] with the
pseudo core τ minus with the all-electron one, which the engine pairs with n_c (design note X2, M0.9). With --abo, the
run's run.abo adds forces_ha_bohr, ABINIT's last "cartesian forces (hartree/bohr) at end" (its cell's own Cartesian
frame, the mean removed), and force_mean_ha_bohr, the "avg" of the line after them: the mean that ABINIT removed
(m_forces.F90), printed to 4 digits.
usage: python3 -I reduce.py [--abo <run.abo>] <reference.json> <bands> <Z>:<count> ... > <name>.json"""
import json
import sys
from pathlib import Path

TERMS = ["kinetic_energy_ha", "hartree_energy_ha", "xc_energy_ha", "ewald_energy_ha", "psp_core_energy_ha",
         "local_energy_ha", "one_center_energy_ha", "smearing_energy_ha"]


def ordered(values):
    """The sum left to right in float64, as Python before 3.12 sums floats (3.12's sum is compensated): the same bytes
    from any interpreter."""
    total = 0.0
    for value in values:
        total += value
    return total


def offsets():
    table = json.loads((Path(__file__).parent / "exccore.json").read_text())
    pseudo, ae = table["pseudo_core_tau_ha"], table["all_electron_core_tau_ha"]
    return {int(z): pseudo[z] - ae[z] for z in pseudo}


def forces(abo):
    """The last block of Cartesian forces in Ha/Bohr of a run.abo, one row per atom, and the mean ABINIT removed."""
    lines = Path(abo).read_text().splitlines()
    start = max(i for i, line in enumerate(lines) if line.strip() == "cartesian forces (hartree/bohr) at end:")
    rows, i = [], start + 1
    while not lines[i].strip().startswith("frms,max,avg="):
        rows.append([float(x) for x in lines[i].split()[1:4]])
        i += 1
    mean = [float(x) for x in lines[i].split("=")[1].split()[2:5]]
    return rows, mean


args = sys.argv[1:]
abo = None
if args[0] == "--abo":
    abo, args = args[1], args[2:]
source = json.loads(Path(args[0]).read_text())
bands = int(args[1])
counts = [tuple(int(x) for x in a.split(":")) for a in args[2:]]
delta = offsets()
out = {key: source[key] for key in TERMS}
out["free_energy_ha"] = ordered(source[key] for key in TERMS)
out["core_offset_ha"] = ordered(delta[z] * n for z, n in counts)
out["fermi_level_ha"] = source["fermi_level_ha"]
out["kpoints_fractional"] = source["kpoints_fractional"]
out["eigenvalues_ha"] = [[k[:bands] for k in spin] for spin in source["eigenvalues_ha"]]
if "magnetization_bohr_magneton" in source:
    out["magnetization_bohr_magneton"] = source["magnetization_bohr_magneton"]
if abo is not None:
    out["forces_ha_bohr"], out["force_mean_ha_bohr"] = forces(abo)
print(json.dumps(out, indent=1))
