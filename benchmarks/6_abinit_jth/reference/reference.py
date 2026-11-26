"""Reduce an ABINIT run of run.sh to the numbers that case 6 compares: energy terms, eigenvalues, k-points, and with
two spin channels the cell's magnetization, Σ_k w Σ_band (f↑ − f↓) (ABINIT's "exact up − dn").
usage: python -I reference.py <run dir> > <name>.json   (reads runo_GSR.nc; prints one JSON object)"""
import json, pathlib, sys
import h5py
import numpy as np
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
