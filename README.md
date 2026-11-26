<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/brand/logo-dark.svg">
    <img src="assets/brand/logo-light.svg" alt="orbitaR Quantum" width="490">
  </picture>
</p>

# orbitaR

**Quantum simulation at native speed, powered by Rust.**

A high-performance electronic structure engine written entirely in Rust.

- **Performance**: Engineered for speed and scalability.
- **Efficiency**: Memory-efficient computation by design.
- **Accuracy**: orbitaR-PBE, our own PAW dataset (coming soon).
- **Functionals**: LDA, PBE, and r²SCAN exchange–correlation functionals.

## Architecture

```text
1  Theory          Kohn–Sham DFT
|
2  Functional      PBE/LDA · r2SCAN
|
3  Representation  Plane Waves (PW)
|
4  Core treatment  Projector Augmented-Wave (PAW-XML · orbitaR-PBE)
|
5  Implementation  orbitaR — Rust
```

## Install

```sh
pip install orbitar
```

## ASE

```python
from ase.build import bulk
from orbitar.ase import Orbitar

atoms = bulk("Si", "diamond", a=5.43)
atoms.calc = Orbitar(kpoints={"mesh": [2, 2, 2]})  # PBE (default)
# atoms.calc = Orbitar(xc="r2scan")                # r2SCAN

# get energy, forces, stress
energy = atoms.get_potential_energy()
forces = atoms.get_forces()
stress = atoms.get_stress()
```

## Input schema

<details>
<summary>Every table, key and option of schema 1 but [probe]</summary>

```toml
# Schema 1 but [probe]: every table, key, and option. Units sit in key names. Unknown ones fail.
# default: the same as writing it; left out: what the engine does without the key.
schema = 1                                 # required

[structure]                                # required
cell_bohr = [[0.0, 5.13, 5.13],            # required; rows are the lattice vectors a1, a2, a3 (Bohr)
             [5.13, 0.0, 5.13],
             [5.13, 5.13, 0.0]]
species = ["Si", "Si"]                     # default []: no atoms, the empty lattice (H = T)
positions_fractional = [[0.0, 0.0, 0.0],   # one row per species entry, in units of a1, a2, a3
                        [0.25, 0.25, 0.25]]

[basis]                                    # required
cutoff_ha = 15.0                           # required; plane waves with |k+G|²/2 ≤ cutoff_ha
density_cutoff_ha = 30.0                   # left out: 2 cutoff_ha; at least cutoff_ha; densities keep |G|² ≤ 8 × it

[kpoints]                                  # required: mesh, or list
mesh = [8, 8, 8]                           # Γ-centered divisions along b1, b2, b3
shift = [0.5, 0.5, 0.5]                    # default [0.0, 0.0, 0.0]; in divisions, each in [0, 1)
symmetry = true                            # default true: with atoms, a mesh solves only its irreducible points
# list = [[0.0, 0.0, 0.0], [0.5, 0.0, 0.5]]   instead of mesh: fractional k-points, each in [-1, 1], used as given

[bands]                                    # required without atoms
count = 16                                 # left out with atoms: ⌈N/2⌉; with a smearing it grows as needed
# options: "lobpcg" | "dense"
solver = "lobpcg"                          # default "lobpcg"

[xc]
# options: "lda" | "pbe" | "r2scan"
functional = "pbe"                         # left out with atoms: "pbe"

[datasets]
family = "jth-2.0"                         # default "jth-2.0"; directory/<family>.toml names each file; r2scan: pbe
directory = "datasets"                     # default "datasets"; beside the input; a file off its sha256 is invalid
files = { Si = "own/Si.xml" }              # default {}; element → its file in directory; family gives the others

[spin]
moments = [2.5, -2.5]                      # left out: no spin; collinear, a start moment (μB) per atom, |m| ≤ valence

[occupations]
# options: "fixed" | "fermi-dirac" | "gaussian" | "methfessel-paxton" | "marzari-vanderbilt"
smearing = "fermi-dirac"                   # left out: "fixed"
width_ha = 0.00225                         # required for every smearing but "fixed", which takes none

[scf]
energy_tolerance_ha = 1e-9                 # default 1e-9; stop: ‖Δñ‖_L2, r2scan ‖Δτ̃‖_L2 < it/100, max |Δρ_ij| < it/10
max_iterations = 100                       # default 100
# options: "linear" | "pulay" | "broyden"
mixer = "pulay"                            # default "pulay"; the others exit 3

[outputs]
forces = true                              # default false; adds forces_ha_bohr
stress = true                              # default false; stress_ha_bohr3 at fixed plane waves; ε: a → (1 + ε) a
step_energies = true                       # default false; each SCF step on stderr: scf_step <n> free_energy_ha <F>
discretization = true                      # default false; adds discretization
[parallel]
threads = 4                                # left out: every core; at least 1; no output depends on it
```

</details>

## License

- [PolyForm Noncommercial 1.0.0](LICENSE): source-available, free for noncommercial use
- [GPL-3.0](datasets/orbitaR-PBE-v0/LICENSE): `datasets/orbitaR-PBE-v0/`, its ATOMPAW inputs and its PAW-XML files
- [GPL-3.0-only](packages/orbitar-jth/LICENSE): the JTH v2.0 PAW-XML files of the `orbitar-jth` package
