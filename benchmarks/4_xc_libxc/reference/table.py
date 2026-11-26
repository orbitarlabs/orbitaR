"""Case 4 reference: Libxc energy densities, their partials, ε, and their spreads at the points of each case input,
and the unit test's strict fixtures.

usage: python -I table.py <cache dir> > table.json.new   (SOURCE.toml names the interpreter, zstandard, and packages)

The script downloads each pinned conda-forge Libxc build into <cache dir> once, checks its sha256 at every run, unpacks
it afresh, and calls the library through ctypes.
- r2scan: values, ε (Libxc's zk), spreads, and cancellation factors from Libxc 6.2.2, at the points as given and
  Libxc's own thresholds; the fixtures block too.
- lda, pbe: e and partials are data from the PySCF 2.14.0 table of 923012c (Linux, Libxc 7.0.0), copied unchanged with
  the top-level `libxc`; their ε, spreads, and cancellation factors come from the osx-arm64 Libxc 7.0.0 build at the
  engine's floored point.
A spread is, per row and output, the largest |change| of that output when one input moves by one unit in the last
place, up or down, one input at a time, over the neighbours with the centre's pattern of finite outputs and exact
zeros: a neighbour whose outputs are not all finite, or whose exact zeros differ from the centre's (a screen, a dropped
spin, a part switched off), does not enter. That pattern is not the branch: a neighbour can cross a clamp and keep it
(the fourth review's row 39, ρ↑ one ulp up, puts σ↑↓ at its cap). A cancellation factor κ is, per row and part,
|e_LDA| / |e|: how far the part's energy cancels against the LDA part it builds on.
"""
import ctypes
import hashlib
import json
import math
import pathlib
import shutil
import sys
import tarfile
import tomllib
import urllib.request
import zipfile

import zstandard

# conda-forge osx-arm64 builds of the Libxc C library: the .conda file and its sha256. 6.2.2 is the version that the X2
# ABINIT 10.0.3 reference build links.
PACKAGES = {
    "6.2.2": ("libxc-c-6.2.2-cpu_h5d0f620_6", "3b3b409a227779e337c68aba7d931fcc9a77d223ddcde8cb1d800eeeb1c23dae"),
    "7.0.0": ("libxc-c-7.0.0-cpu_h7ee64da_6", "48447188ab20b8c65d1a6c696246a56129a34ad07dcec68eae6d18a340cc9780"),
}
NAMES = {
    "lda": ["LDA_X", "LDA_C_PW"],
    "pbe": ["GGA_X_PBE", "GGA_C_PBE"],
    "r2scan": ["MGGA_X_R2SCAN", "MGGA_C_R2SCAN"],
}
PARTS = ("exchange", "correlation")
# The engine's floors of the lda and pbe rows (rho_s 1e-14, tau_s 1e-20), with Libxc's density and sigma thresholds
# set below them, as the table.py of 923012c called Libxc.
DENSITY_FLOOR, TAU_FLOOR, BELOW_FLOOR, SIGMA_BELOW = 1e-14, 1e-20, 1e-20, 1e-40
# The rule of tests/xc/functional.rs, written into table.json for it. An exact zero of Libxc's admits nothing else, a
# NaN only a NaN; otherwise a value meets Libxc's within (1e-12 + CANCELLATION κ u) |want| + SPREADS spread, plus
# DENSITY u |ε| on a density partial, u = 2^-53, κ and ε the part's. C κ u is relative, the part's energy standing in for
# each value's cancellation (κ rounded up); D u |ε| is absolute (v = ε + n ∂ε/∂n rounds with |ε| whatever its own size)
# and applies to strict parts too; a strict part drops the spread term alone. A part whose C κ u exceeds 1e-12 on a row
# is out of the case's energy measure (`<part>_case`), never out of the unit test. The three factors are empirical, set
# 2026-11-24 (SOURCE.toml names the rows).
SPREADS, CANCELLATION, DENSITY, U = 4, 16, 8, 2.0**-53
# The LDA parts that a part's cancellation is measured against: κ = |e_LDA| / |e| at the same point.
LDA = {"exchange": "LDA_X", "correlation": "LDA_C_PW_MOD"}
# Libxc 6.2.2's density thresholds of r2SCAN (mgga_x_r2scan.c:49, mgga_c_r2scan.c:47) and the share below which its
# 1 ∓ ζ, formed by subtraction, moves by ulp(1) when an input moves by one ulp: a r2scan row is `strict` there, and where
# its total density equals a threshold.
THRESHOLDS, SHARE = {"exchange": 1e-11, "correlation": 1e-15}, 2.0**-26
# The relative distance of a fixture's side rows from an equality where the value read scales with an input: one ulp
# would put a long number within science S1's 1e-6 of the engine constant at the equality. `relation` checks each side.
SIDE = 2e-6
HERE = pathlib.Path(__file__).resolve().parent
CASE = HERE.parent


def library(cache, version):
    """The Libxc shared library of `version`, unpacked afresh from its archive in `cache` after its sha256 matches."""
    package, sha = PACKAGES[version]
    archive = cache / f"{package}.conda"
    if not archive.exists():
        urllib.request.urlretrieve(f"https://conda.anaconda.org/conda-forge/osx-arm64/{package}.conda", archive)
    assert hashlib.sha256(archive.read_bytes()).hexdigest() == sha, f"{archive}: sha256 differs from {sha}"
    folder = cache / package
    shutil.rmtree(folder, ignore_errors=True)
    with zipfile.ZipFile(archive) as conda, conda.open(f"pkg-{package}.tar.zst") as packed:
        with zstandard.ZstdDecompressor().stream_reader(packed) as stream:
            with tarfile.open(fileobj=stream, mode="r|") as tar:
                tar.extractall(folder, filter="data")
    so = ctypes.CDLL(str(folder / "lib" / "libxc.dylib"))
    so.xc_func_alloc.restype = ctypes.c_void_p
    so.xc_functional_get_number.argtypes = [ctypes.c_char_p]
    so.xc_version_string.restype = ctypes.c_char_p
    assert so.xc_version_string().decode() == version, folder
    return so


def doubles(values):
    return (ctypes.c_double * len(values))(*values)


def libxc(so, name, rho, sigma, tau, below=False):
    """Libxc's [e = (Σ rho) ε, ∂e/∂rho…, ∂e/∂sigma…, ∂e/∂tau…, ε] at one point, spin-polarized for two densities and
    unpolarized for one, 0 for a variable the functional does not read; at Libxc's own thresholds, or `below` the
    engine's floors."""
    spins = len(rho)
    func = ctypes.c_void_p(so.xc_func_alloc())
    assert so.xc_func_init(func, so.xc_functional_get_number(name.encode()), spins) == 0, name
    if below:
        so.xc_func_set_dens_threshold(func, ctypes.c_double(BELOW_FLOOR))
        so.xc_func_set_sigma_threshold(func, ctypes.c_double(SIGMA_BELOW))
    zk, vrho, vlapl, vtau = doubles([0.0]), doubles([0.0] * spins), doubles([0.0] * spins), doubles([0.0] * spins)
    vsigma = doubles([0.0] * len(sigma))
    one = ctypes.c_size_t(1)
    if name.startswith("MGGA"):
        so.xc_mgga_exc_vxc(func, one, doubles(rho), doubles(sigma), doubles([0.0] * spins), doubles(tau), zk, vrho, vsigma, vlapl, vtau)
    elif name.startswith("GGA"):
        so.xc_gga_exc_vxc(func, one, doubles(rho), doubles(sigma), zk, vrho, vsigma)
    else:
        so.xc_lda_exc_vxc(func, one, doubles(rho), zk, vrho)
    so.xc_func_end(func)
    so.xc_func_free(func)
    return [zk[0] * sum(rho), *vrho, *vsigma, *vtau, zk[0]]


def spread(f, inputs):
    """Per output of `f`, the largest |change| when one input moves by one unit in the last place, up or down, over the
    neighbours with the centre's pattern: all outputs finite, and exact zeros where the centre's are."""
    base = f(inputs)
    zeros = [x == 0.0 for x in base]
    worst = [0.0] * len(base)
    for k, x in enumerate(inputs):
        for direction in (math.inf, -math.inf):
            moved = list(inputs)
            moved[k] = math.nextafter(x, direction)
            out = f(moved)
            if all(math.isfinite(y) for y in out) and [y == 0.0 for y in out] == zeros:
                worst = [max(w, abs(a - b)) for w, a, b in zip(worst, out, base)]
    return worst


def shipped(so, name):
    """A function of the seven polarized inputs: Libxc's row at its own thresholds, ε last."""
    return lambda q: libxc(so, name, q[0:2], q[2:5], q[5:7])


def unpolarized(so, name):
    """A function of (n, σ, τ): Libxc's unpolarized row at its own thresholds, ε last."""
    return lambda x: libxc(so, name, [x[0]], [x[1]], [x[2]])


def floored(point):
    """ρ↑, ρ↓, σ↑↑, σ↑↓, σ↓↓, τ↑, τ↓ of an lda or pbe point raised to the engine's floors."""
    tau = point[5:7] if len(point) == 7 else [0.0, 0.0]
    return [max(r, DENSITY_FLOOR) for r in point[0:2]] + list(point[2:5]) + [max(t, TAU_FLOOR) for t in tau]


def below(so, name):
    """A function of the seven floored inputs: Libxc's polarized row with its thresholds below the floors, ε last."""
    return lambda q: libxc(so, name, q[0:2], q[2:5], q[5:7], True)


def scaled(so, name):
    """A function of the seven floored inputs: the exchange from Libxc's unpolarized path through exact spin scaling,
    E_x = ½ E_x[2ρ↑, 4σ↑↑, 2τ↑] + ½ E_x[2ρ↓, 4σ↓↓, 2τ↓], in the layout of a polarized row (no ε)."""

    def row(q):
        halves = [libxc(so, name, [2 * r], [4 * s], [2 * t], True) for r, s, t in zip(q[0:2], [q[2], q[4]], q[5:7])]
        e = sum(h[0] for h in halves) / 2
        vrho = [h[1] for h in halves]
        vsigma = [2 * halves[0][2], 0.0, 2 * halves[1][2]]
        vtau = [h[3] for h in halves] if name.startswith("MGGA") else [0.0] * 2
        return [e, *vrho, *vsigma, *vtau]

    return row


def holds(got, want, spread_, kappa, epsilon, column):
    """The rule (see SPREADS) for a value in `column` of a polarized row: an exact zero of `want` first."""
    if want == 0.0:
        return got == 0.0
    allowed = (1e-12 + CANCELLATION * (kappa or 0.0) * U) * abs(want) + SPREADS * spread_
    if column in (1, 2):
        allowed += DENSITY * U * abs(epsilon)
    return abs(got - want) <= allowed


def cancellation(e, e_lda):
    """κ = |e_LDA| / |e| of a part, rounded up to two significant digits (a bound needs no more, and a long number
    near 1 would read as an engine constant to science S1); None where Libxc's e is an exact zero (the exact-zero rule
    applies)."""
    if e == 0.0 or not math.isfinite(e):
        return None
    kappa = abs(e_lda) / abs(e)
    if kappa == 0.0:
        return 0.0
    exponent = math.floor(math.log10(kappa)) - 1
    return float(f"{math.ceil(kappa / 10.0**exponent)}e{exponent}")


def in_case(kappa):
    """A part's `case` flag: its C κ u within 1e-12 (an exact zero, no κ, is measured as one)."""
    return kappa is None or CANCELLATION * kappa * U <= 1e-12


def strict(part, point):
    """Whether a part of a polarized r2scan point is `strict`: its total density at the part's threshold, or a prepared
    minority share 2ρ_s/ρ (each ρ_s raised to the correlation's threshold) below SHARE, where Libxc's 1 ∓ ζ is quantized
    by its subtraction and the engine reproduces ζ and its powers bit for bit."""
    total = point[0] + point[1]
    if total == THRESHOLDS[part]:
        return True
    if total < THRESHOLDS["correlation"]:
        return False
    up, down = (max(r, THRESHOLDS["correlation"]) for r in point[0:2])
    return 2 * min(up, down) / (up + down) < SHARE


def strict_unpolarized(part, x):
    """Whether a part of an unpolarized r2scan point is `strict`: n at the part's threshold, or for the exchange n/2."""
    return x[0] == THRESHOLDS[part] or (part == "exchange" and x[0] / 2 == THRESHOLDS[part])


def block(so, points, function, names):
    """Values, ε, spreads, κ, and case flags of each part at `points` through `function(so, name)`."""
    out = {}
    for part, name in zip(PARTS, names):
        f, g = function(so, name), function(so, LDA[part])
        rows = [f(p) for p in points]
        out[part] = [r[:-1] for r in rows]
        out[f"{part}_epsilon"] = [r[-1] for r in rows]
        spreads = [spread(f, p) for p in points]
        out[f"{part}_spread"] = [s[:-1] for s in spreads]
        out[f"{part}_epsilon_spread"] = [s[-1] for s in spreads]
        kappa = [cancellation(r[0], g(p)[0]) for r, p in zip(rows, points)]
        out[f"{part}_kappa"] = kappa
        out[f"{part}_case"] = [in_case(k) for k in kappa]
    return out


def nulls(row):
    """A row with each NaN as None (JSON null: NaN expected)."""
    return [None if math.isnan(x) else x for x in row]


cache = pathlib.Path(sys.argv[1]).resolve()
table = json.loads((HERE / "table.json").read_text())

# lda and pbe: the stored e and partials stay; the 7.0.0 build gives their ε, spreads, and κ, and its values must meet
# the stored ones by the rule.
old = library(cache, "7.0.0")
for word in ("lda", "pbe"):
    # The stored keys of 923012c alone, in their order; this script writes the rest afresh.
    stored = ("libxc", "points", "exchange", "exchange_scaled", "correlation")
    rows = {key: table["functionals"][word][key] for key in stored}
    table["functionals"][word] = rows
    floor = [floored(p) for p in rows["points"]]
    for part, name in zip(PARTS, NAMES[word]):
        f = below(old, name)
        ours = [f(q) for q in floor]
        spreads = [spread(f, q) for q in floor]
        kappa = [cancellation(rows[part][i][0], below(old, LDA[part])(q)[0]) for i, q in enumerate(floor)]
        rows[f"{part}_epsilon"] = [o[-1] for o in ours]
        rows[f"{part}_spread"] = [s[:-1] for s in spreads]
        rows[f"{part}_epsilon_spread"] = [s[-1] for s in spreads]
        rows[f"{part}_kappa"] = kappa
        rows[f"{part}_case"] = [in_case(k) for k in kappa]
        for i in range(len(floor)):
            for c, (a, b, s) in enumerate(zip(ours[i], rows[part][i], spreads[i])):
                if c < 8:
                    good = holds(a, b, s, kappa[i], ours[i][-1], c)
                    assert good, f"{word} {part} row {i + 1} column {c}: 7.0.0 builds {a} and {b}, spread {s}"
    f = scaled(old, NAMES[word][0])
    spreads = [spread(f, q) for q in floor]
    rows["exchange_scaled_spread"] = spreads
    for i, q in enumerate(floor):
        for c, (a, b, s) in enumerate(zip(f(q), rows["exchange_scaled"][i], spreads[i])):
            good = holds(a, b, s, rows["exchange_kappa"][i], rows["exchange_epsilon"][i], c)
            assert good, f"{word} scaled row {i + 1} column {c}: {a} and {b}"

# r2scan: everything from 6.2.2.
so = library(cache, "6.2.2")
flat = lambda name: (lambda e: e(1.0) == e(2.0) == e(4.0))(lambda s: libxc(so, name, [0.1], [s * 0.08], [0.01])[0])
assert all(flat(name) for name in NAMES["r2scan"]), "Libxc 6.2.2 lacks the Fermi-hole clamp"
points = tomllib.loads((CASE / "inputs" / "r2scan.toml").read_text())["probe"]["points"]
# The unpolarized point of each: both spins as its spin up, (n, σ, τ) = (2ρ↑, 4σ↑↑, 2τ↑).
spinless = [[2 * p[0], 4 * p[2], 2 * p[5]] for p in points]
r2scan = {"libxc": NAMES["r2scan"], "version": "6.2.2", "inputs": "shipped", "points": points}
r2scan.update(block(so, points, shipped, NAMES["r2scan"]))
for part in PARTS:
    r2scan[f"{part}_strict"] = [strict(part, p) for p in points]
spin = {"points": spinless}
spin.update(block(so, spinless, unpolarized, NAMES["r2scan"]))
for part in PARTS:
    spin[f"{part}_strict"] = [strict_unpolarized(part, x) for x in spinless]
r2scan["unpolarized"] = spin


# The fixtures: rows that only the unit test reads, each part strict: by the rule with no spread, so (1e-12 + C κ u)
# |want|, plus D u |ε| on a density partial, exact zeros and NaN first. Each comparison of Libxc's input convention
# (src/xc/libxc.rs) gets rows on each side of its edge and at it, built in binary64, with the relation of the value it
# reads asserted below; an equality that cannot change an output gets a reason (SOURCE.toml) and no row of its own.
def sides(value):
    """The equality and a row SIDE below and above it."""
    return [value * (1 - SIDE), value, value * (1 + SIDE)]


def relation(values, edge, want, what):
    """Assert that each of `values` compares with `edge` as `want` names it ("<", "==", ">")."""
    got = ["<" if v < edge else "==" if v == edge else ">" for v in values]
    assert got == list(want), f"{what}: {got}, not {want}, at {edge}"


def sigma_floor(part):
    """Libxc's σ floor of a part: its sigma_threshold = dens_threshold^{4/3}, squared (work_mgga_inc.c)."""
    threshold = THRESHOLDS[part] ** (4.0 / 3.0)
    return threshold * threshold


def prepared(part, q):
    """The point that Libxc 6.2.2 hands the part's formula (work_mgga_inc.c), as src/xc/libxc.rs forms it."""
    thr, floor = THRESHOLDS[part], sigma_floor(part)
    spins = []
    for rho, sigma, tau in ((q[0], q[2], q[5]), (q[1], q[4], q[6])):
        rho, tau = max(rho, thr), max(tau, TAU_FLOOR)
        spins.append((rho, min(max(sigma, floor), 8.0 * rho * tau), tau))
    (up, uu, tu), (down, dd, td) = spins
    return [up, down, uu, q[3], dd, tu, td]


def minus(q):
    """1 − ζ of a prepared point as Libxc's generated code rounds it: 1 − (ρ↑ − ρ↓) (1/ρ)."""
    return 1.0 - (q[0] - q[1]) * (1.0 / (q[0] + q[1]))


def small(q):
    """The exchange's share of the down spin, 2ρ↓ (1/ρ), of a prepared point."""
    return q[1] * 2.0 * (1.0 / (q[0] + q[1]))


EPS = sys.float_info.epsilon
BACKGROUND = [0.3, 0.2, 0.02, 0.005, 0.01, 0.1, 0.08]
polarized = []
for part, thr in THRESHOLDS.items():
    # The total-density screen of each part, and the exchange's per-spin drop (one spin empty).
    rows = [[r, 0.0, 0.0, 0.0, 0.0, 1e-25, 0.0] for r in sides(thr)]
    relation([q[0] + q[1] for q in rows], thr, ["<", "==", ">"], f"{part} screen")
    polarized += rows
    # A spin's density floor beside an ordinary spin; for the exchange, its drop at ρ↓ <= thr after the floor.
    rows = [[0.3, r, 0.02, 0.0, 0.0, 0.1, 1e-12] for r in sides(thr)]
    relation([q[1] for q in rows], thr, ["<", "==", ">"], f"{part} density floor")
    relation([prepared(part, q)[1] for q in rows], thr, ["==", "==", ">"], f"{part} drop")
    polarized += rows
for part, thr in THRESHOLDS.items():
    # The σ floor σ_thr², σ_thr = thr^{4/3} squared as Libxc and the engine square it: one spin just above the exchange's
    # threshold.
    rows = [[3e-11, 2e-11, s, 0.0, 0.0, 1e-17, 8e-18] for s in sides((thr ** (4.0 / 3.0)) ** 2)]
    relation([q[2] for q in rows], sigma_floor(part), ["<", "==", ">"], f"{part} σ floor")
    relation([8.0 * q[0] * q[5] for q in rows], sigma_floor(part), [">"] * 3, f"{part} σ floor under the cap")
    polarized += rows
# The correlation's σ floor where it moves the correlation's outputs: both spins near its density threshold (at 3e-11
# above, an output does not resolve σ near 1e-40).
rows = [[1.5e-15, 1e-15, s, 0.0, 0.0, 1e-17, 8e-18] for s in sides((THRESHOLDS["correlation"] ** (4.0 / 3.0)) ** 2)]
relation([q[2] for q in rows], sigma_floor("correlation"), ["<", "==", ">"], "correlation σ floor near its threshold")
polarized += rows
# The Fermi-hole cap σ_ss = 8ρ_sτ_s of each spin.
for s, (rho, tau) in ((2, (0, 5)), (4, (1, 6))):
    cap = 8.0 * BACKGROUND[rho] * BACKGROUND[tau]
    rows = [BACKGROUND[:s] + [x] + BACKGROUND[s + 1:] for x in sides(cap)]
    for part in PARTS:
        relation([q[s] for q in rows], 8.0 * prepared(part, rows[1])[rho] * prepared(part, rows[1])[tau], ["<", "==", ">"], f"{part} cap {s}")
    polarized += rows
# The τ floor, at densities where both parts keep both spins; σ under its floor, so the cap 8ρτ sets it from τ.
rows = [[3e-11, 2e-11, 1e-32, 0.0, 1e-32, t, 1e-19] for t in sides(TAU_FLOOR)]
relation([q[5] for q in rows], TAU_FLOOR, ["<", "==", ">"], "τ floor")
assert all(min(q[0], q[1]) > THRESHOLDS["exchange"] for q in rows), "τ floor: a spin the exchange drops"
polarized += rows
# The cross term at ±s_ave, s_ave from the prepared diagonal.
rows = [[*BACKGROUND[0:3], x, *BACKGROUND[4:]] for x in sides(0.5 * (BACKGROUND[2] + BACKGROUND[4]))]
rows += [[*BACKGROUND[0:3], -x, *BACKGROUND[4:]] for x in sides(0.5 * (BACKGROUND[2] + BACKGROUND[4]))]
for part in PARTS:
    average = (lambda q: 0.5 * (q[2] + q[4]))(prepared(part, BACKGROUND))
    relation([q[3] for q in rows[:3]], average, ["<", "==", ">"], f"{part} cross +")
    relation([q[3] for q in rows[3:]], -average, [">", "==", "<"], f"{part} cross −")
polarized += rows
# The exchange's small-spin share 2ρ_s (1/ρ) at DBL_EPSILON (libxc.rs `small`), its edge found from Libxc's arithmetic.
def at_epsilon(minority, value):
    """The majority density at which `value(majority)` first reaches DBL_EPSILON or less, by bisection on floats."""
    low, high = minority, minority * 1e20
    while math.nextafter(low, math.inf) < high:
        middle = math.sqrt(low * high) if high / low > 2 else 0.5 * (low + high)
        if middle in (low, high):
            break
        (high, low) = (middle, low) if value(middle) <= EPS else (high, middle)
    return high


share = lambda minority: (lambda up: 2.0 * minority * (1.0 / (up + minority)))
up = at_epsilon(2e-11, share(2e-11))
rows = [[u, 2e-11, 0.1, 0.0, 0.0, 1.0, 1.0] for u in sides(up)]
relation([small(prepared("exchange", q)) for q in rows], EPS, [">", "==", "<"], "exchange small share")
polarized += rows
# The correlation's 1 − ζ near DBL_EPSILON by bisection on the same arithmetic: it lands on 1.5ε, ε, 1.5ε (1 − ζ is
# quantized there), so the equality alone; these rows set C (SOURCE.toml).
rows = [[u, 1e-13, 0.1, 0.0, 0.0, 1.0, 1.0] for u in sides(at_epsilon(1e-13, lambda u: minus([u, 1e-13])))]
relation([minus(prepared("correlation", q)) for q in rows], EPS, [">", "==", ">"], "correlation 1 − ζ, quantized")
polarized += rows
# 1 − ζ itself below, at, and above DBL_EPSILON, majority densities chosen in binary64: the floor of the generated code
# (libxc.rs `floored`), in the exchange's share of a kept spin (ρ↓ above its threshold, `small` false) and in the
# correlation, whose φ takes (1 − ζ)^{2/3}; the mirror of the correlation's equality holds 1 + ζ.
rows = [[u, 2e-11, 8e14, 0.0, 1e-30, 2e9, 1e-15] for u in (111672.0, 83630.0, 65640.0)]
relation([minus(prepared("exchange", q)) for q in rows], EPS, ["<", "==", ">"], "exchange 1 − ζ")
relation([small(prepared("exchange", q)) for q in rows], EPS, [">"] * 3, "exchange 1 − ζ, a kept share")
polarized += rows
rows = [[u, 1e-14, 1e6, 0.0, 0.0, 5e3, 1e-20] for u in (55.4, 42.4, 40.1)]
relation([minus(prepared("correlation", q)) for q in rows], EPS, ["<", "==", ">"], "correlation 1 − ζ")
polarized += rows
mirror = [1e-14, 42.4, 0.0, 0.0, 1e6, 1e-20, 5e3]
relation([1.0 + (lambda q: (q[0] - q[1]) * (1.0 / (q[0] + q[1])))(prepared("correlation", mirror))], EPS, ["=="], "1 + ζ")
polarized.append(mirror)
# The zero guard of x^{5/3} (libxc.rs `five_thirds`) on the share (1 − ζ)/2: 0 here, above 0 at the 55.4 row; below 0
# is not reached (SOURCE.toml).
zero = [128.1, 1e-14, 1e6, 0.0, 0.0, 5e3, 1e-20]
relation([minus(prepared("correlation", q)) * 0.5 for q in (zero, rows[0])], 0.0, ["==", ">"], "five_thirds")
polarized.append(zero)
# The review's accepted points where Libxc's correlation is NaN (its third sweep, cross term near its clamp).
polarized += [
    [1.1620248631743837e-12, 1.0361690241136306e-13, 5.775764811166151e-13, -5.775764811166152e-14, 5.775764811166151e-13, 0.012424939911586353, 1.2767484585208432e-05],
    [1.245581827949567e-12, 5.493291143827205e-16, 4.207952496324872e-12, -4.2079524963248717e-13, 4.207952496324872e-12, 0.0844534309595241, 0.005153496945211119],
    [1.7451203353716325e-12, 1.409882201255363e-13, 5.152497087106248e-13, -5.152497087106248e-14, 5.152497087106248e-13, 0.007381291969262463, 6.315236495434907e-12],
]
# The fourth review's points where p of the exchange rounds to 0 and its exponent overflows (r2scan.rs `flat_gx`), with
# Libxc's exchange partials finite: n near 1e110 and σ = 0 as there, τ near τ_unif (at its τ, 0.5 and 1, α is 0 and the
# correlation is −b1c to every digit, an engine constant to science S1).
flat = [5e110, 5e110, 0.0, 0.0, 0.0, 5e183, 5e183]
assert all(math.isfinite(v) for v in shipped(so, NAMES["r2scan"][0])(flat)), "flat_gx point"
polarized.append(flat)
spinless = []
for part, thr in THRESHOLDS.items():
    rows = [[n, 0.0, 1e-25] for n in sides(thr)]
    relation([x[0] for x in rows], thr, ["<", "==", ">"], f"unpolarized {part} screen")
    spinless += rows
# The exchange's unpolarized drop at n/2 = its threshold.
rows = [[n, 0.0, 1e-25] for n in sides(2 * THRESHOLDS["exchange"])]
relation([x[0] / 2 for x in rows], THRESHOLDS["exchange"], ["<", "==", ">"], "unpolarized drop")
spinless += rows
# The unpolarized σ floor of each part (n near the part's threshold, where an output resolves it), the cap σ = 8nτ, and
# the τ floor where the exchange keeps n.
for (part, thr), n in zip(THRESHOLDS.items(), (3e-11, 2e-15)):
    rows = [[n, s, 1e-17] for s in sides((thr ** (4.0 / 3.0)) ** 2)]
    relation([x[1] for x in rows], sigma_floor(part), ["<", "==", ">"], f"unpolarized {part} σ floor")
    spinless += rows
rows = [[0.5, s, 0.1] for s in sides(8.0 * 0.5 * 0.1)]
relation([x[1] for x in rows], 8.0 * 0.5 * 0.1, ["<", "==", ">"], "unpolarized cap")
spinless += rows
rows = [[5e-11, 1e-32, t] for t in sides(TAU_FLOOR)]
relation([x[2] for x in rows], TAU_FLOOR, ["<", "==", ">"], "unpolarized τ floor")
assert all(x[0] / 2 > THRESHOLDS["exchange"] for x in rows), "unpolarized τ floor: an n the exchange drops"
spinless += rows
flat = [1e110, 0.0, 1e184]
assert all(math.isfinite(v) for v in unpolarized(so, NAMES["r2scan"][0])(flat)), "unpolarized flat_gx point"
spinless.append(flat)
fixtures = {"polarized": {"points": polarized}, "unpolarized": {"points": spinless}}
for layout, rows, function in (("polarized", polarized, shipped), ("unpolarized", spinless, unpolarized)):
    values = block(so, rows, function, NAMES["r2scan"])
    for part in PARTS:
        values[part] = [nulls(v) for v in values[part]]
        values[f"{part}_epsilon"] = [None if math.isnan(v) else v for v in values[f"{part}_epsilon"]]
        del values[f"{part}_case"]
    fixtures[layout].update(values)
r2scan["fixtures"] = fixtures
table["functionals"]["r2scan"] = r2scan
table["rule"] = {"spreads": SPREADS, "cancellation": CANCELLATION, "density": DENSITY}
print(json.dumps(table, indent=1))
