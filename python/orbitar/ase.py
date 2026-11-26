import json
import math
import subprocess
from importlib.metadata import entry_points
from pathlib import Path

import numpy as np
from ase.calculators.calculator import kptdensity2monkhorstpack
from ase.calculators.genericfileio import BaseProfile, CalculatorTemplate, GenericFileIOCalculator
from ase.calculators.socketio import actualunixsocketname
from ase.stress import full_3x3_to_voigt_6_stress
from ase.units import Bohr, Hartree

from orbitar.datasets import rows

SCALARS = {
    "cutoff_ha": ("basis", "cutoff_ha"),
    "density_cutoff_ha": ("basis", "density_cutoff_ha"),
    "bands": ("bands", "count"),
    "solver": ("bands", "solver"),
    "xc": ("xc", "functional"),
    "datasets": ("datasets", "directory"),
    "dataset_files": ("datasets", "files"),
    "threads": ("parallel", "threads"),
}
KNOWN = {*SCALARS, "kpoints", "smearing", "scf"}
FAMILY = "jth-2.0"
GROUP = "orbitar.datasets"


class OrbitarProfile(BaseProfile):

    def __init__(self, command="orbitar"):
        super().__init__(command)

    def get_calculator_command(self, inputfile):
        return ["run", inputfile]

    def version(self):
        line = subprocess.run([*self._split_command, "--version"], capture_output=True, text=True, check=True).stdout
        return line.strip().removeprefix("orbitar ")


class OrbitarTemplate(CalculatorTemplate):
    inputname = "input.toml"
    outputname = "outcome.json"
    errorname = "stderr.log"

    def __init__(self):
        super().__init__("orbitar", ["energy", "free_energy", "forces", "stress"])

    def write_input(self, profile, directory, atoms, parameters, properties):
        parameters = defaults(atoms, parameters, Path(directory), profile._split_command)
        text = input_toml(atoms, parameters, forces="forces" in properties, stress="stress" in properties)
        Path(directory, self.inputname).write_text(text)

    def execute(self, directory, profile):
        profile.run(Path(directory), self.inputname, self.outputname, errorfile=self.errorname)

    def read_results(self, directory):
        outcome = json.loads(Path(directory, self.outputname).read_text())
        free = outcome["free_energy_ha"] * Hartree
        results = {"energy": free, "free_energy": free}
        if "forces_ha_bohr" in outcome:
            results["forces"] = np.array(outcome["forces_ha_bohr"]) * (Hartree / Bohr)
        if "stress_ha_bohr3" in outcome:
            stress_ev_per_angstrom3 = np.array(outcome["stress_ha_bohr3"]) * (Hartree / Bohr**3)
            results["stress"] = full_3x3_to_voigt_6_stress(stress_ev_per_angstrom3)
        return results

    def socketio_argv(self, profile, unixsocket, port):
        if port is not None or unixsocket is None:
            raise ValueError("Orbitar: orbitar ipi takes a Unix socket (unixsocket=...), not a port")
        return ["ipi", "--unix", actualunixsocketname(unixsocket), self.inputname]

    def socketio_parameters(self, unixsocket, port):
        return {}

    def load_profile(self, cfg, **kwargs):
        return OrbitarProfile.from_config(cfg, self.name, **kwargs)


class Orbitar(GenericFileIOCalculator):

    def __init__(self, *, profile=None, directory=".", **parameters):
        unknown = set(parameters) - KNOWN
        if unknown:
            raise ValueError(f"Orbitar: unknown parameters {sorted(unknown)}; known: {sorted(KNOWN)}")
        super().__init__(
            template=OrbitarTemplate(),
            profile=profile or OrbitarProfile(),
            directory=directory,
            parameters=parameters,
        )


def defaults(atoms, parameters, directory, orbitar):
    found = dict(parameters)
    if not found.get("dataset_files"):
        found.pop("dataset_files", None)
    elements = sorted(set(atoms.get_chemical_symbols()))
    settled = "datasets" in found or "dataset_files" in found
    if elements and not settled and not (directory / "datasets" / f"{FAMILY}.toml").is_file():
        package = installed(FAMILY)
        if package is None:
            raise ValueError(
                f"Orbitar: no dataset directory for family {FAMILY}: no datasets= given, no datasets/{FAMILY}.toml "
                f"beside the input, and no installed data package (entry point group {GROUP}); "
                f"pip install orbitar-jth, pass datasets=<directory>, or run "
                f"`python -m orbitar.datasets fetch datasets` beside the input"
            )
        found["datasets"] = str(package)
    if "cutoff_ha" not in found:
        found["cutoff_ha"] = hint(elements, found, directory, orbitar)
    if "kpoints" not in found:
        found["kpoints"] = {"mesh": [int(n) for n in kptdensity2monkhorstpack(atoms)]}
    return found


def installed(family):
    for point in entry_points(group=GROUP, name=family):
        return Path(point.load().__file__).parent
    return None


MAKER = {"lda": "lda", "pbe": "pbe", "r2scan": "pbe"}


def family_rows(parameters, directory, orbitar):
    xc = parameters.get("xc", "pbe")
    functional = MAKER.get(xc, xc)
    try:
        table = rows(directory / parameters.get("datasets", "datasets"), None, orbitar)
    except ValueError as refusal:
        raise ValueError(f"Orbitar: no cutoff_ha, and the family gives no hint: {refusal}") from None
    return {row["element"]: row for row in table if row["functional"] == functional}


def hint(elements, parameters, directory, orbitar):
    if not elements:
        raise ValueError("Orbitar: the cell holds no atoms, so no element gives a cutoff hint; pass cutoff_ha")
    own = sorted(set(elements) & set(parameters.get("dataset_files", {})))
    if own:
        raise ValueError(f"Orbitar: dataset_files names {own}, so the family gives no cutoff hint; pass cutoff_ha")
    read = family_rows(parameters, directory, orbitar)
    hints = {element: read.get(element, {}).get("cutoff_hint_ha") for element in elements}
    missing = [element for element in elements if hints[element] is None]
    if missing:
        xc = parameters.get("xc", "pbe")
        raise ValueError(
            f"Orbitar: the family {FAMILY} gives no {MAKER.get(xc, xc)} cutoff hint (xc {xc}) for {missing}; "
            "pass cutoff_ha"
        )
    return max(hints.values())


def input_toml(atoms, parameters, forces, stress):
    if not all(atoms.pbc):
        raise ValueError("Orbitar: orbitaR runs periodic cells only; set pbc=True on every axis")
    tables = {"structure": {
        "cell_bohr": (np.asarray(atoms.cell) / Bohr).tolist(),
        "species": atoms.get_chemical_symbols(),
        "positions_fractional": atoms.get_scaled_positions(wrap=False).tolist(),
    }}
    for name, (table, key) in SCALARS.items():
        if name in parameters:
            tables.setdefault(table, {})[key] = parameters[name]
    if "kpoints" in parameters:
        tables["kpoints"] = dict(parameters["kpoints"])
    if "smearing" in parameters:
        name, width = parameters["smearing"]
        tables["occupations"] = {"smearing": name, "width_ha": width}
    if "scf" in parameters:
        tables["scf"] = dict(parameters["scf"])
    moments = atoms.get_initial_magnetic_moments()
    if np.any(moments != 0.0):
        tables["spin"] = {"moments": moments.tolist()}
    outputs = {key: True for key, asked in (("forces", forces), ("stress", stress)) if asked}
    if outputs:
        tables["outputs"] = outputs
    lines = ["schema = 1"]
    for table, keys in tables.items():
        lines += ["", f"[{table}]"] + [f"{key} = {value_toml(value)}" for key, value in keys.items()]
    return "\n".join(lines) + "\n"


def value_toml(value):
    if isinstance(value, (bool, np.bool_)):
        return "true" if value else "false"
    if isinstance(value, (int, np.integer)):
        return str(int(value))
    if isinstance(value, (float, np.floating)):
        if not math.isfinite(value):
            raise ValueError(f"Orbitar: {value} is not a finite number")
        return repr(float(value))
    if isinstance(value, str):
        return json.dumps(value)
    if isinstance(value, (list, tuple, np.ndarray)):
        return "[" + ", ".join(value_toml(v) for v in value) + "]"
    if isinstance(value, dict):
        return "{" + ", ".join(f"{json.dumps(str(k))} = {value_toml(v)}" for k, v in value.items()) + "}"
    raise TypeError(f"Orbitar: no TOML form for {value!r}")
