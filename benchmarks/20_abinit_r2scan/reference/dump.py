import json
import sys
import xml.etree.ElementTree as ElementTree
from pathlib import Path

USAGE = """usage: python3 -I dump.py json <run dir> <atom> <dataset.xml> <element> > <name>.json
       python3 -I dump.py toml <name>.json > ../inputs/<name>.toml"""


def rows(path):
    return [line.split() for line in Path(path).read_text().splitlines() if line and not line.startswith("#")]


def number(text):
    return float(text.replace("D", "E"))


def engine_orbital_index(dataset):
    states = ElementTree.parse(dataset).getroot().find("valence_states")
    index, seen = {}, {}
    for state in states:
        l = int(state.get("l"))
        seen[l] = seen.get(l, 0) + 1
        for m in range(-l, l + 1):
            index[(l, m, seen[l])] = len(index)
    return index


def from_dump(directory, atom, dataset, element):
    index = engine_orbital_index(dataset)
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
    if matrices([(int(r[1]), int(r[2]), number(r[3])) for r in dij if r[0] == "rhoij"]) != rho:
        sys.exit("pawdij and pawdenpot read different rho_ij: the dump holds two states, not one")
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


if sys.argv[1:2] == ["json"] and len(sys.argv) == 6:
    print(json.dumps(from_dump(sys.argv[2], int(sys.argv[3]), sys.argv[4], sys.argv[5]), indent=1))
elif sys.argv[1:2] == ["toml"] and len(sys.argv) == 3:
    print(toml(sys.argv[2]))
else:
    sys.exit(USAGE)
