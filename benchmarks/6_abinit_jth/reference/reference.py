import json, pathlib, sys
import h5py
import numpy as np
USAGE = "usage: python -I reference.py <run dir> > <name>.json"
if len(sys.argv) != 2:
    sys.exit(USAGE)
run = pathlib.Path(sys.argv[1])
with h5py.File(run / "runo_GSR.nc", "r") as h:
    get = lambda k: np.array(h[k][()])
    out = {
        "kinetic_energy_ha": float(get("e_kinetic")),
        "hartree_energy_ha": float(get("e_hartree")),
        "xc_energy_ha": float(get("e_xc")),
        "ewald_energy_ha": float(get("e_ewald")),
        "psp_core_energy_ha": float(get("e_corepsp")),
        "local_energy_ha": float(get("e_localpsp")),
        "one_center_energy_ha": float(get("e_paw")),
        "smearing_energy_ha": float(get("e_entropy")),
        "free_energy_ha": float(get("etotal")),
        "fermi_level_ha": float(get("e_fermie")),
        "kpoints_fractional": get("reduced_coordinates_of_kpoints").tolist(),
        "eigenvalues_ha": get("eigenvalues").tolist(),
    }
    occupations = get("occupations")
    if len(occupations) == 2:
        weights = get("kpoint_weights")
        out["magnetization_bohr_magneton"] = float((weights[:, None] * (occupations[0] - occupations[1])).sum())
print(json.dumps(out))
