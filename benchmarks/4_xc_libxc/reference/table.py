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

CONDA_FORGE_OSX_ARM64_LIBXC = {
    "6.2.2": ("libxc-c-6.2.2-cpu_h5d0f620_6", "3b3b409a227779e337c68aba7d931fcc9a77d223ddcde8cb1d800eeeb1c23dae"),
    "7.0.0": ("libxc-c-7.0.0-cpu_h7ee64da_6", "48447188ab20b8c65d1a6c696246a56129a34ad07dcec68eae6d18a340cc9780"),
}
NAMES = {
    "lda": ["LDA_X", "LDA_C_PW"],
    "pbe": ["GGA_X_PBE", "GGA_C_PBE"],
    "r2scan": ["MGGA_X_R2SCAN", "MGGA_C_R2SCAN"],
}
PARTS = ("exchange", "correlation")
ENGINE_DENSITY_FLOOR, TAU_FLOOR, DENSITY_THRESHOLD_BELOW_FLOORS, SIGMA_THRESHOLD_BELOW_FLOORS = 1e-14, 1e-20, 1e-20, 1e-40
K_SPREADS, C_CANCELLATION, D_DENSITY, UNIT_ROUNDOFF = 4, 16, 8, 2.0**-53
CANCELS_AGAINST_LDA = {"exchange": "LDA_X", "correlation": "LDA_C_PW_MOD"}
R2SCAN_DENSITY_THRESHOLDS, QUANTIZED_MINORITY_SHARE = {"exchange": 1e-11, "correlation": 1e-15}, 2.0**-26
SIDE_RELATIVE_OFFSET = 2e-6
USAGE = "usage: python -I table.py <cache dir> > table.json.new"
HERE = pathlib.Path(__file__).resolve().parent
CASE = HERE.parent


def unpacked_libxc(cache, version):
    package, sha = CONDA_FORGE_OSX_ARM64_LIBXC[version]
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


def libxc(so, name, rho, sigma, tau, with_thresholds_below_floors=False):
    spins = len(rho)
    func = ctypes.c_void_p(so.xc_func_alloc())
    assert so.xc_func_init(func, so.xc_functional_get_number(name.encode()), spins) == 0, name
    if with_thresholds_below_floors:
        so.xc_func_set_dens_threshold(func, ctypes.c_double(DENSITY_THRESHOLD_BELOW_FLOORS))
        so.xc_func_set_sigma_threshold(func, ctypes.c_double(SIGMA_THRESHOLD_BELOW_FLOORS))
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


def ulp_spread(f, inputs):
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


def at_libxc_thresholds(so, name):
    return lambda q: libxc(so, name, q[0:2], q[2:5], q[5:7])


def unpolarized(so, name):
    return lambda x: libxc(so, name, [x[0]], [x[1]], [x[2]])


def raised_to_engine_floors(point):
    tau = point[5:7] if len(point) == 7 else [0.0, 0.0]
    return [max(r, ENGINE_DENSITY_FLOOR) for r in point[0:2]] + list(point[2:5]) + [max(t, TAU_FLOOR) for t in tau]


def with_thresholds_below_floors(so, name):
    return lambda q: libxc(so, name, q[0:2], q[2:5], q[5:7], True)


def exchange_by_spin_scaling(so, name):

    def row(q):
        halves = [libxc(so, name, [2 * r], [4 * s], [2 * t], True) for r, s, t in zip(q[0:2], [q[2], q[4]], q[5:7])]
        e = sum(h[0] for h in halves) / 2
        vrho = [h[1] for h in halves]
        vsigma = [2 * halves[0][2], 0.0, 2 * halves[1][2]]
        vtau = [h[3] for h in halves] if name.startswith("MGGA") else [0.0] * 2
        return [e, *vrho, *vsigma, *vtau]

    return row


def meets_rule(got, want, spread_, kappa, epsilon, column):
    if want == 0.0:
        return got == 0.0
    allowed = (1e-12 + C_CANCELLATION * (kappa or 0.0) * UNIT_ROUNDOFF) * abs(want) + K_SPREADS * spread_
    if column in (1, 2):
        allowed += D_DENSITY * UNIT_ROUNDOFF * abs(epsilon)
    return abs(got - want) <= allowed


def kappa_rounded_up(e, e_lda):
    if e == 0.0 or not math.isfinite(e):
        return None
    kappa = abs(e_lda) / abs(e)
    if kappa == 0.0:
        return 0.0
    exponent = math.floor(math.log10(kappa)) - 1
    return float(f"{math.ceil(kappa / 10.0**exponent)}e{exponent}")


def in_case(kappa):
    return kappa is None or C_CANCELLATION * kappa * UNIT_ROUNDOFF <= 1e-12


def strict(part, point):
    total = point[0] + point[1]
    if total == R2SCAN_DENSITY_THRESHOLDS[part]:
        return True
    if total < R2SCAN_DENSITY_THRESHOLDS["correlation"]:
        return False
    up, down = (max(r, R2SCAN_DENSITY_THRESHOLDS["correlation"]) for r in point[0:2])
    return 2 * min(up, down) / (up + down) < QUANTIZED_MINORITY_SHARE


def strict_unpolarized(part, x):
    return x[0] == R2SCAN_DENSITY_THRESHOLDS[part] or (part == "exchange" and x[0] / 2 == R2SCAN_DENSITY_THRESHOLDS[part])


def block(so, points, function, names):
    out = {}
    for part, name in zip(PARTS, names):
        f, g = function(so, name), function(so, CANCELS_AGAINST_LDA[part])
        rows = [f(p) for p in points]
        out[part] = [r[:-1] for r in rows]
        out[f"{part}_epsilon"] = [r[-1] for r in rows]
        spreads = [ulp_spread(f, p) for p in points]
        out[f"{part}_spread"] = [s[:-1] for s in spreads]
        out[f"{part}_epsilon_spread"] = [s[-1] for s in spreads]
        kappa = [kappa_rounded_up(r[0], g(p)[0]) for r, p in zip(rows, points)]
        out[f"{part}_kappa"] = kappa
        out[f"{part}_case"] = [in_case(k) for k in kappa]
    return out


def nan_as_null(row):
    return [None if math.isnan(x) else x for x in row]


if len(sys.argv) != 2:
    sys.exit(USAGE)
cache = pathlib.Path(sys.argv[1]).resolve()
table = json.loads((HERE / "table.json").read_text())

old = unpacked_libxc(cache, "7.0.0")
for word in ("lda", "pbe"):
    stored = ("libxc", "points", "exchange", "exchange_scaled", "correlation")
    rows = {key: table["functionals"][word][key] for key in stored}
    table["functionals"][word] = rows
    floor = [raised_to_engine_floors(p) for p in rows["points"]]
    for part, name in zip(PARTS, NAMES[word]):
        f = with_thresholds_below_floors(old, name)
        ours = [f(q) for q in floor]
        spreads = [ulp_spread(f, q) for q in floor]
        kappa = [kappa_rounded_up(rows[part][i][0], with_thresholds_below_floors(old, CANCELS_AGAINST_LDA[part])(q)[0]) for i, q in enumerate(floor)]
        rows[f"{part}_epsilon"] = [o[-1] for o in ours]
        rows[f"{part}_spread"] = [s[:-1] for s in spreads]
        rows[f"{part}_epsilon_spread"] = [s[-1] for s in spreads]
        rows[f"{part}_kappa"] = kappa
        rows[f"{part}_case"] = [in_case(k) for k in kappa]
        for i in range(len(floor)):
            for c, (a, b, s) in enumerate(zip(ours[i], rows[part][i], spreads[i])):
                if c < 8:
                    good = meets_rule(a, b, s, kappa[i], ours[i][-1], c)
                    assert good, f"{word} {part} row {i + 1} column {c}: 7.0.0 builds {a} and {b}, spread {s}"
    f = exchange_by_spin_scaling(old, NAMES[word][0])
    spreads = [ulp_spread(f, q) for q in floor]
    rows["exchange_scaled_spread"] = spreads
    for i, q in enumerate(floor):
        for c, (a, b, s) in enumerate(zip(f(q), rows["exchange_scaled"][i], spreads[i])):
            good = meets_rule(a, b, s, rows["exchange_kappa"][i], rows["exchange_epsilon"][i], c)
            assert good, f"{word} scaled row {i + 1} column {c}: {a} and {b}"

so = unpacked_libxc(cache, "6.2.2")
flat = lambda name: (lambda e: e(1.0) == e(2.0) == e(4.0))(lambda s: libxc(so, name, [0.1], [s * 0.08], [0.01])[0])
assert all(flat(name) for name in NAMES["r2scan"]), "Libxc 6.2.2 lacks the Fermi-hole clamp"
points = tomllib.loads((CASE / "inputs" / "r2scan.toml").read_text())["probe"]["points"]
spinless = [[2 * p[0], 4 * p[2], 2 * p[5]] for p in points]
r2scan = {"libxc": NAMES["r2scan"], "version": "6.2.2", "inputs": "shipped", "points": points}
r2scan.update(block(so, points, at_libxc_thresholds, NAMES["r2scan"]))
for part in PARTS:
    r2scan[f"{part}_strict"] = [strict(part, p) for p in points]
spin = {"points": spinless}
spin.update(block(so, spinless, unpolarized, NAMES["r2scan"]))
for part in PARTS:
    spin[f"{part}_strict"] = [strict_unpolarized(part, x) for x in spinless]
r2scan["unpolarized"] = spin


def sides(value):
    return [value * (1 - SIDE_RELATIVE_OFFSET), value, value * (1 + SIDE_RELATIVE_OFFSET)]


def assert_relations(values, edge, want, what):
    got = ["<" if v < edge else "==" if v == edge else ">" for v in values]
    assert got == list(want), f"{what}: {got}, not {want}, at {edge}"


def sigma_floor(part):
    threshold = R2SCAN_DENSITY_THRESHOLDS[part] ** (4.0 / 3.0)
    return threshold * threshold


def as_libxc_prepares(part, q):
    thr, floor = R2SCAN_DENSITY_THRESHOLDS[part], sigma_floor(part)
    spins = []
    for rho, sigma, tau in ((q[0], q[2], q[5]), (q[1], q[4], q[6])):
        rho, tau = max(rho, thr), max(tau, TAU_FLOOR)
        spins.append((rho, min(max(sigma, floor), 8.0 * rho * tau), tau))
    (up, uu, tu), (down, dd, td) = spins
    return [up, down, uu, q[3], dd, tu, td]


def one_minus_zeta(q):
    return 1.0 - (q[0] - q[1]) * (1.0 / (q[0] + q[1]))


def exchange_minority_share(q):
    return q[1] * 2.0 * (1.0 / (q[0] + q[1]))


EPS = sys.float_info.epsilon
BACKGROUND = [0.3, 0.2, 0.02, 0.005, 0.01, 0.1, 0.08]
polarized = []
for part, thr in R2SCAN_DENSITY_THRESHOLDS.items():
    rows = [[r, 0.0, 0.0, 0.0, 0.0, 1e-25, 0.0] for r in sides(thr)]
    assert_relations([q[0] + q[1] for q in rows], thr, ["<", "==", ">"], f"{part} screen")
    polarized += rows
    rows = [[0.3, r, 0.02, 0.0, 0.0, 0.1, 1e-12] for r in sides(thr)]
    assert_relations([q[1] for q in rows], thr, ["<", "==", ">"], f"{part} density floor")
    assert_relations([as_libxc_prepares(part, q)[1] for q in rows], thr, ["==", "==", ">"], f"{part} drop")
    polarized += rows
for part, thr in R2SCAN_DENSITY_THRESHOLDS.items():
    rows = [[3e-11, 2e-11, s, 0.0, 0.0, 1e-17, 8e-18] for s in sides((thr ** (4.0 / 3.0)) ** 2)]
    assert_relations([q[2] for q in rows], sigma_floor(part), ["<", "==", ">"], f"{part} σ floor")
    assert_relations([8.0 * q[0] * q[5] for q in rows], sigma_floor(part), [">"] * 3, f"{part} σ floor under the cap")
    polarized += rows
rows = [[1.5e-15, 1e-15, s, 0.0, 0.0, 1e-17, 8e-18] for s in sides((R2SCAN_DENSITY_THRESHOLDS["correlation"] ** (4.0 / 3.0)) ** 2)]
assert_relations([q[2] for q in rows], sigma_floor("correlation"), ["<", "==", ">"], "correlation σ floor near its threshold")
polarized += rows
for s, (rho, tau) in ((2, (0, 5)), (4, (1, 6))):
    cap = 8.0 * BACKGROUND[rho] * BACKGROUND[tau]
    rows = [BACKGROUND[:s] + [x] + BACKGROUND[s + 1:] for x in sides(cap)]
    for part in PARTS:
        assert_relations([q[s] for q in rows], 8.0 * as_libxc_prepares(part, rows[1])[rho] * as_libxc_prepares(part, rows[1])[tau], ["<", "==", ">"], f"{part} cap {s}")
    polarized += rows
rows = [[3e-11, 2e-11, 1e-32, 0.0, 1e-32, t, 1e-19] for t in sides(TAU_FLOOR)]
assert_relations([q[5] for q in rows], TAU_FLOOR, ["<", "==", ">"], "τ floor")
assert all(min(q[0], q[1]) > R2SCAN_DENSITY_THRESHOLDS["exchange"] for q in rows), "τ floor: a spin the exchange drops"
polarized += rows
rows = [[*BACKGROUND[0:3], x, *BACKGROUND[4:]] for x in sides(0.5 * (BACKGROUND[2] + BACKGROUND[4]))]
rows += [[*BACKGROUND[0:3], -x, *BACKGROUND[4:]] for x in sides(0.5 * (BACKGROUND[2] + BACKGROUND[4]))]
for part in PARTS:
    average = (lambda q: 0.5 * (q[2] + q[4]))(as_libxc_prepares(part, BACKGROUND))
    assert_relations([q[3] for q in rows[:3]], average, ["<", "==", ">"], f"{part} cross +")
    assert_relations([q[3] for q in rows[3:]], -average, [">", "==", "<"], f"{part} cross −")
polarized += rows
def majority_at_epsilon(minority, value):
    low, high = minority, minority * 1e20
    while math.nextafter(low, math.inf) < high:
        middle = math.sqrt(low * high) if high / low > 2 else 0.5 * (low + high)
        if middle in (low, high):
            break
        (high, low) = (middle, low) if value(middle) <= EPS else (high, middle)
    return high


share = lambda minority: (lambda up: 2.0 * minority * (1.0 / (up + minority)))
up = majority_at_epsilon(2e-11, share(2e-11))
rows = [[u, 2e-11, 0.1, 0.0, 0.0, 1.0, 1.0] for u in sides(up)]
assert_relations([exchange_minority_share(as_libxc_prepares("exchange", q)) for q in rows], EPS, [">", "==", "<"], "exchange small share")
polarized += rows
rows = [[u, 1e-13, 0.1, 0.0, 0.0, 1.0, 1.0] for u in sides(majority_at_epsilon(1e-13, lambda u: one_minus_zeta([u, 1e-13])))]
assert_relations([one_minus_zeta(as_libxc_prepares("correlation", q)) for q in rows], EPS, [">", "==", ">"], "correlation 1 − ζ, quantized")
polarized += rows
rows = [[u, 2e-11, 8e14, 0.0, 1e-30, 2e9, 1e-15] for u in (111672.0, 83630.0, 65640.0)]
assert_relations([one_minus_zeta(as_libxc_prepares("exchange", q)) for q in rows], EPS, ["<", "==", ">"], "exchange 1 − ζ")
assert_relations([exchange_minority_share(as_libxc_prepares("exchange", q)) for q in rows], EPS, [">"] * 3, "exchange 1 − ζ, a kept share")
polarized += rows
rows = [[u, 1e-14, 1e6, 0.0, 0.0, 5e3, 1e-20] for u in (55.4, 42.4, 40.1)]
assert_relations([one_minus_zeta(as_libxc_prepares("correlation", q)) for q in rows], EPS, ["<", "==", ">"], "correlation 1 − ζ")
polarized += rows
mirror = [1e-14, 42.4, 0.0, 0.0, 1e6, 1e-20, 5e3]
assert_relations([1.0 + (lambda q: (q[0] - q[1]) * (1.0 / (q[0] + q[1])))(as_libxc_prepares("correlation", mirror))], EPS, ["=="], "1 + ζ")
polarized.append(mirror)
zero = [128.1, 1e-14, 1e6, 0.0, 0.0, 5e3, 1e-20]
assert_relations([one_minus_zeta(as_libxc_prepares("correlation", q)) * 0.5 for q in (zero, rows[0])], 0.0, ["==", ">"], "five_thirds")
polarized.append(zero)
polarized += [
    [1.1620248631743837e-12, 1.0361690241136306e-13, 5.775764811166151e-13, -5.775764811166152e-14, 5.775764811166151e-13, 0.012424939911586353, 1.2767484585208432e-05],
    [1.245581827949567e-12, 5.493291143827205e-16, 4.207952496324872e-12, -4.2079524963248717e-13, 4.207952496324872e-12, 0.0844534309595241, 0.005153496945211119],
    [1.7451203353716325e-12, 1.409882201255363e-13, 5.152497087106248e-13, -5.152497087106248e-14, 5.152497087106248e-13, 0.007381291969262463, 6.315236495434907e-12],
]
flat = [5e110, 5e110, 0.0, 0.0, 0.0, 5e183, 5e183]
assert all(math.isfinite(v) for v in at_libxc_thresholds(so, NAMES["r2scan"][0])(flat)), "flat_gx point"
polarized.append(flat)
spinless = []
for part, thr in R2SCAN_DENSITY_THRESHOLDS.items():
    rows = [[n, 0.0, 1e-25] for n in sides(thr)]
    assert_relations([x[0] for x in rows], thr, ["<", "==", ">"], f"unpolarized {part} screen")
    spinless += rows
rows = [[n, 0.0, 1e-25] for n in sides(2 * R2SCAN_DENSITY_THRESHOLDS["exchange"])]
assert_relations([x[0] / 2 for x in rows], R2SCAN_DENSITY_THRESHOLDS["exchange"], ["<", "==", ">"], "unpolarized drop")
spinless += rows
for (part, thr), n in zip(R2SCAN_DENSITY_THRESHOLDS.items(), (3e-11, 2e-15)):
    rows = [[n, s, 1e-17] for s in sides((thr ** (4.0 / 3.0)) ** 2)]
    assert_relations([x[1] for x in rows], sigma_floor(part), ["<", "==", ">"], f"unpolarized {part} σ floor")
    spinless += rows
rows = [[0.5, s, 0.1] for s in sides(8.0 * 0.5 * 0.1)]
assert_relations([x[1] for x in rows], 8.0 * 0.5 * 0.1, ["<", "==", ">"], "unpolarized cap")
spinless += rows
rows = [[5e-11, 1e-32, t] for t in sides(TAU_FLOOR)]
assert_relations([x[2] for x in rows], TAU_FLOOR, ["<", "==", ">"], "unpolarized τ floor")
assert all(x[0] / 2 > R2SCAN_DENSITY_THRESHOLDS["exchange"] for x in rows), "unpolarized τ floor: an n the exchange drops"
spinless += rows
flat = [1e110, 0.0, 1e184]
assert all(math.isfinite(v) for v in unpolarized(so, NAMES["r2scan"][0])(flat)), "unpolarized flat_gx point"
spinless.append(flat)
fixtures = {"polarized": {"points": polarized}, "unpolarized": {"points": spinless}}
for layout, rows, function in (("polarized", polarized, at_libxc_thresholds), ("unpolarized", spinless, unpolarized)):
    values = block(so, rows, function, NAMES["r2scan"])
    for part in PARTS:
        values[part] = [nan_as_null(v) for v in values[part]]
        values[f"{part}_epsilon"] = [None if math.isnan(v) else v for v in values[f"{part}_epsilon"]]
        del values[f"{part}_case"]
    fixtures[layout].update(values)
r2scan["fixtures"] = fixtures
table["functionals"]["r2scan"] = r2scan
table["rule"] = {"spreads": K_SPREADS, "cancellation": C_CANCELLATION, "density": D_DENSITY}
print(json.dumps(table, indent=1))
