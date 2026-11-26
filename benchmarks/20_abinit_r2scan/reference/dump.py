"""One paw probe of case 20 from ABINIT's same-state dump (abinit-paw-dump.patch, reference/SOURCE.toml): the atom's
rho_ij as pawdenpot read it, e1xc and etild1xc of that call, and dijxc of each spin from the pawdij call after it, each
matrix in the engine's orbital order (the dataset's channels in file order, m = -l..l in each), which ABINIT's
indlmn (l, m, n) gives: n counts the channels of one l in file order.
usage: python3 -I dump.py json <run dir> <atom> <dataset.xml> <element> > <name>.json
       python3 -I dump.py toml <name>.json > ../inputs/<name>.toml"""
import json
import sys
import xml.etree.ElementTree as ElementTree
from pathlib import Path


def rows(path):
    return [line.split() for line in Path(path).read_text().splitlines() if line and not line.startswith("#")]


def number(text):
    return float(text.replace("D", "E"))


def engine_index(dataset):
    """(l, m, n) -> the engine's orbital index, from the dataset's valence states in file order."""
    states = ElementTree.parse(dataset).getroot().find("valence_states")
    index, seen = {}, {}
    for state in states:
        l = int(state.get("l"))
        seen[l] = seen.get(l, 0) + 1
        for m in range(-l, l + 1):
            index[(l, m, seen[l])] = len(index)
    return index


def from_dump(directory, atom, dataset, element):
    index = engine_index(dataset)
    size = len(index)
    denpot = rows(f"{directory}/PAW_DUMP_DENPOT_{atom}.txt")
    dij = rows(f"{directory}/PAW_DUMP_DIJ_{atom}.txt")
    orbital = {int(r[1]): index[(int(r[2]), int(r[3]), int(r[4]))] for r in denpot if r[0] == "indlmn"}
    pair = {int(r[1]): (orbital[int(r[2])], orbital[int(r[3])]) for r in denpot if r[0] == "klmn"}
    energy = {r[0]: number(r[1]) for r in denpot if r[0] in ("e1xc", "etild1xc")}

    def matrices(entries):
        spins = max(spin for spin, _, _ in entries)
        out = [[[0.0] * size for _ in range(size)] for _ in range(spins)]
        for spin, klmn, value in entries:
            i, j = pair[klmn]
            out[spin - 1][i][j] = out[spin - 1][j][i] = value
        return out

    rho = matrices([(int(r[1]), int(r[2]), number(r[3])) for r in denpot if r[0] == "rhoij"])
    d = matrices([(int(r[1]), int(r[2]), number(r[3])) for r in dij if r[0] == "dijxc"])
    # pawdij read the rho_ij that pawdenpot read: one state.
    if matrices([(int(r[1]), int(r[2]), number(r[3])) for r in dij if r[0] == "rhoij"]) != rho:
        sys.exit("the two files hold different rho_ij")
    return {
        "element": element,
        "rho_ij": rho,
        "paw_xc_energy_ha": [energy["e1xc"], energy["etild1xc"]],
        "paw_xc_d_ha": d,
    }


def toml(reference):
    rho = json.loads(Path(reference).read_text())
    element = rho["element"]
    lines = ["schema = 1", "", "[structure]", "cell_bohr = [[10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]]", "",
             "[basis]", "cutoff_ha = 1.0", "", "[kpoints]", "list = [[0.0, 0.0, 0.0]]", "", "[xc]",
             'functional = "r2scan"', "", "[probe]",
             'term = "paw"', "", "[probe.rho_ij]", f"{element} = ["]
    for spin in rho["rho_ij"]:
        lines.append("  [")
        lines += [f"    [{', '.join(repr(x) for x in row)}]," for row in spin]
        lines.append("  ],")
    lines.append("]")
    return "\n".join(lines)


if sys.argv[1] == "json":
    print(json.dumps(from_dump(sys.argv[2], int(sys.argv[3]), sys.argv[4], sys.argv[5]), indent=1))
else:
    print(toml(sys.argv[2]))
