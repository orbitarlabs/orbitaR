use super::{
    Outcome, Row,
    claim::{Claim, ORBITAR, Pair, Series, Versus},
    escaped, log10, px,
};
use std::{collections::BTreeSet, fmt::Write as _};

pub(super) const CARDS: [&str; 8] = [
    "assets/benchmarks/card-speed-light.svg",
    "assets/benchmarks/card-speed-dark.svg",
    "assets/benchmarks/card-memory-light.svg",
    "assets/benchmarks/card-memory-dark.svg",
    "assets/benchmarks/card-scaling-light.svg",
    "assets/benchmarks/card-scaling-dark.svg",
    "assets/benchmarks/card-verification-light.svg",
    "assets/benchmarks/card-verification-dark.svg",
];

const WIDTH: f64 = 960.0;
const PAD: f64 = 48.0;

type Colour = [&'static str; 2];

struct Palette {
    surface: Colour,
    border: Colour,
    text: Colour,
    muted: Colour,
    grid: Colour,
    track: Colour,
    orbitar: Colour,
    qe: Colour,
    abinit: Colour,
}

const PALETTE: Palette = Palette {
    surface: ["#f6f4ef", "#101112"],
    border: ["#e3dfd5", "#2c2d2f"],
    text: ["#171817", "#f6f4ef"],
    muted: ["#6b6964", "#a6a39c"],
    grid: ["#e3dfd5", "#2c2d2f"],
    track: ["#ece8df", "#1c1d1f"],
    orbitar: ["#bf6038", "#eb965f"],
    qe: ["#2f7f78", "#5cbfb3"],
    abinit: ["#5b5f97", "#a0a5e3"],
};

fn colour(code: &str) -> Colour {
    match code {
        ORBITAR => PALETTE.orbitar,
        "qe" => PALETTE.qe,
        "abinit" => PALETTE.abinit,
        _ => PALETTE.muted,
    }
}

fn named(code: &str) -> String {
    match code {
        ORBITAR => "orbitaR".into(),
        rival => rival.to_uppercase(),
    }
}

pub(super) fn cards(rows: &[Row]) -> Vec<(&'static str, Option<String>)> {
    let drawn: [Option<Box<dyn Card + '_>>; 4] = [
        bars(rows, Metric::Speed).map(|c| Box::new(c) as Box<dyn Card>),
        bars(rows, Metric::Memory).map(|c| Box::new(c) as Box<dyn Card>),
        scaling(rows).map(|c| Box::new(c) as Box<dyn Card>),
        verification(rows).map(|c| Box::new(c) as Box<dyn Card>),
    ];
    let mut out = Vec::new();
    for (paths, card) in CARDS.chunks(2).zip(drawn) {
        for (theme, path) in paths.iter().enumerate() {
            out.push((*path, card.as_ref().map(|c| c.draw(theme))));
        }
    }
    out
}

trait Card {
    fn draw(&self, theme: usize) -> String;
}

fn cut(x: f64) -> String {
    if x.is_infinite() {
        return "∞".into();
    }
    let text = format!("{x}");
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    let digits: String = fraction
        .chars()
        .chain(std::iter::repeat('0'))
        .take(2)
        .collect();
    format!("{whole}.{digits}")
}

fn cells(n: usize) -> String {
    match n {
        1 => "1 cell".into(),
        n => format!("{n} cells"),
    }
}

fn seconds(v: f64) -> String {
    match v {
        v if v.is_infinite() => "lost run".into(),
        v if v < 10.0 => format!("{v:.2} s"),
        v if v < 600.0 => format!("{v:.1} s"),
        v if v < 36_000.0 => format!("{:.1} min", v / 60.0),
        v => format!("{:.1} h", v / 3600.0),
    }
}

fn bytes(v: f64) -> String {
    match v {
        v if v.is_infinite() => "lost run".into(),
        v if v < 1e9 => format!("{:.0} MB", v / 1e6),
        v => format!("{:.2} GB", v / 1e9),
    }
}

fn superscript(text: &str) -> String {
    (text.chars())
        .map(|c| match c {
            '-' => '⁻',
            '0' => '⁰',
            '1' => '¹',
            '2' => '²',
            '3' => '³',
            '4' => '⁴',
            '5' => '⁵',
            '6' => '⁶',
            '7' => '⁷',
            '8' => '⁸',
            '9' => '⁹',
            other => other,
        })
        .collect()
}

fn scientific(v: f64) -> String {
    let text = format!("{v:.1e}");
    match text.split_once('e') {
        Some((mantissa, "0")) => mantissa.to_owned(),
        Some((mantissa, exponent)) => format!("{mantissa} × 10{}", superscript(exponent)),
        None => text,
    }
}

fn stem(input: &str) -> &str {
    let name = input.rsplit('/').next().unwrap_or(input);
    name.strip_suffix(".toml").unwrap_or(name)
}

struct Svg {
    out: String,
    theme: usize,
}

impl Svg {
    fn new(height: f64, theme: usize) -> Svg {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" font-family=\"system-ui, sans-serif\">",
            w = px(WIDTH),
            h = px(height)
        );
        let _ = writeln!(
            out,
            "<rect x=\"0.5\" y=\"0.5\" width=\"{}\" height=\"{}\" rx=\"16\" fill=\"{}\" stroke=\"{}\"/>",
            px(WIDTH - 1.0),
            px(height - 1.0),
            PALETTE.surface[theme],
            PALETTE.border[theme]
        );
        Svg { out, theme }
    }

    fn text(&mut self, x: f64, y: f64, size: f64, style: &str, fill: Colour, body: &str) {
        let _ = writeln!(
            self.out,
            "<text x=\"{}\" y=\"{}\" font-size=\"{}\"{style} fill=\"{}\">{}</text>",
            px(x),
            px(y),
            px(size),
            fill[self.theme],
            escaped(body)
        );
    }

    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64, rx: f64, fill: Colour) {
        let _ = writeln!(
            self.out,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" fill=\"{}\"/>",
            px(x),
            px(y),
            px(w),
            px(h),
            px(rx),
            fill[self.theme]
        );
    }

    fn line(&mut self, from: (f64, f64), to: (f64, f64), stroke: Colour, width: f64) {
        let _ = writeln!(
            self.out,
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"{}\"/>",
            px(from.0),
            px(from.1),
            px(to.0),
            px(to.1),
            stroke[self.theme],
            px(width)
        );
    }

    fn head(&mut self, tag: &str, title: &str, subtitle: &str) {
        self.text(
            PAD,
            56.0,
            13.0,
            " font-weight=\"700\" letter-spacing=\"1.5\"",
            PALETTE.orbitar,
            tag,
        );
        self.text(PAD, 94.0, 30.0, " font-weight=\"650\"", PALETTE.text, title);
        self.text(PAD, 122.0, 15.0, "", PALETTE.muted, subtitle);
    }

    fn heroes(&mut self, heroes: &[Hero]) {
        let width = (WIDTH - 2.0 * PAD) / heroes.len().max(3) as f64;
        for (i, h) in heroes.iter().enumerate() {
            let x = PAD + width * i as f64;
            let power = match h.power.is_empty() {
                true => String::new(),
                false => format!(
                    "<tspan dy=\"-22\" font-size=\"30\">{}</tspan>",
                    escaped(&h.power)
                ),
            };
            let _ = writeln!(
                self.out,
                "<text x=\"{}\" y=\"200.0\" font-size=\"52.0\" font-weight=\"700\" fill=\"{}\">{}{power}</text>",
                px(x),
                PALETTE.orbitar[self.theme],
                escaped(&h.big)
            );
            self.text(
                x,
                228.0,
                16.0,
                " font-weight=\"600\"",
                PALETTE.text,
                &h.line,
            );
            for (j, part) in h.note.split(" · ").enumerate() {
                self.text(x, 250.0 + 18.0 * j as f64, 13.0, "", PALETTE.muted, part);
            }
        }
    }

    fn caption(&mut self, y: f64, lines: &[String]) {
        for (i, line) in lines.iter().enumerate() {
            self.text(PAD, y + 20.0 * i as f64, 13.0, "", PALETTE.muted, line);
        }
    }

    fn end(mut self) -> String {
        self.out.push_str("</svg>\n");
        self.out
    }
}

struct Hero {
    big: String,
    power: String,
    line: String,
    note: String,
}

fn caption(row: &Row, claim: &Claim) -> Vec<String> {
    let threads: BTreeSet<u32> = (claim.versus.iter())
        .flat_map(|v| v.pairs.iter().map(|p| p.threads))
        .collect();
    let list: Vec<String> = threads.iter().map(u32::to_string).collect();
    let cores = match list.is_empty() {
        true => String::new(),
        false => format!("{} cores, ", list.join(", ")),
    };
    let node = claim.cpu().unwrap_or("");
    let commit = row.commit.as_deref().map_or("", |c| &c[..7]);
    let mut builds = vec![format!("case {} · orbitaR at commit {commit}", row.number)];
    for v in &claim.versus {
        if let Ok((version, build)) = claim.package(&v.rival) {
            builds.push(format!("{} {version} ({build})", named(&v.rival)));
        }
    }
    vec![
        format!("{cores}one node, same accuracy target · {node}"),
        builds.join(" · "),
    ]
}

fn omitted(claim: &Claim, memory: bool) -> Vec<String> {
    let mut lines: Vec<String> = (claim.skip.iter())
        .map(|s| {
            format!(
                "not timed: {} {} ({} atoms), {}: {}",
                named(&s.code),
                stem(&s.input),
                s.atoms,
                s.kind.name(),
                s.reason
            )
        })
        .collect();
    if memory {
        for v in &claim.versus {
            for p in v.pairs.iter().filter(|p| !p.in_r) {
                lines.push(format!(
                    "not in R against {}: {}, its runs are too short for a peak Pss reading",
                    named(&v.rival),
                    stem(&p.input)
                ));
            }
        }
    }
    lines
}

fn declared(claim: &Claim) -> BTreeSet<&str> {
    (claim.versus.iter())
        .flat_map(|v| v.pairs.iter().map(|p| p.input.as_str()))
        .chain(claim.skip.iter().map(|s| s.input.as_str()))
        .collect()
}

#[derive(Clone, Copy)]
enum Metric {
    Speed,
    Memory,
}

struct Bars<'a> {
    metric: Metric,
    row: &'a Row,
    claim: &'a Claim,
    input: String,
    heroes: Vec<Hero>,
    bars: Vec<(String, Colour, f64)>,
}

fn pair<'a>(v: &'a Versus, input: &str) -> Option<&'a Pair> {
    v.pairs.iter().find(|p| p.input == input)
}

fn bars(rows: &[Row], metric: Metric) -> Option<Bars<'_>> {
    let (row, claim) = (rows.iter()).find_map(|r| Some((r, r.claim.as_ref()?)))?;
    let first = claim.versus.first()?;
    let read = |p: &Pair| match metric {
        Metric::Speed => true,
        Metric::Memory => p.in_r,
    };
    let mean = |v: &Versus| match metric {
        Metric::Speed => v.g,
        Metric::Memory => v.r,
    };
    let values = |p: &Pair| match metric {
        Metric::Speed => [p.t_delta_s_orbitar, p.t_delta_s_rival],
        Metric::Memory => [p.peak_pss_bytes_orbitar, p.peak_pss_bytes_rival],
    };
    let shared: BTreeSet<&str> = (first.pairs.iter())
        .filter(|p| read(p))
        .map(|p| p.input.as_str())
        .filter(|input| (claim.versus.iter()).all(|v| pair(v, input).is_some_and(read)))
        .collect();
    let distance = |input: &str| -> f64 {
        (claim.versus.iter())
            .map(|v| {
                let [ours, theirs] = values(pair(v, input)?);
                Some((log10(theirs / ours) - log10(mean(v))).abs())
            })
            .map(|d| d.filter(|d| d.is_finite()).unwrap_or(f64::INFINITY))
            .sum()
    };
    let input = *shared
        .iter()
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))?;
    let mut bars = vec![(
        "orbitaR".to_owned(),
        PALETTE.orbitar,
        values(pair(first, input)?)[0],
    )];
    let all = declared(claim).len();
    let mut heroes = Vec::new();
    for v in &claim.versus {
        let label = match claim.package(&v.rival) {
            Ok((version, _)) => format!("{} {version}", named(&v.rival)),
            Err(_) => named(&v.rival),
        };
        bars.push((label, colour(&v.rival), values(pair(v, input)?)[1]));
        let n = v.pairs.iter().filter(|p| read(p)).count();
        let over = format!("geometric mean over {n} of {}", cells(all));
        heroes.push(match metric {
            Metric::Speed => Hero {
                big: format!("≥ {}×", cut(v.g_ci[0])),
                power: String::new(),
                line: format!("faster than {}", named(&v.rival)),
                note: format!("95% CI lower bound · {over}"),
            },
            Metric::Memory => Hero {
                big: format!("{}×", cut(v.r)),
                power: String::new(),
                line: format!("less peak memory than {}", named(&v.rival)),
                note: over,
            },
        });
    }
    Some(Bars {
        metric,
        row,
        claim,
        input: stem(input).to_owned(),
        heroes,
        bars,
    })
}

impl Card for Bars<'_> {
    fn draw(&self, theme: usize) -> String {
        const ROW: f64 = 56.0;
        const BAR: f64 = 38.0;
        const TOP: f64 = 358.0;
        const LEFT: f64 = 240.0;
        const LENGTH: f64 = 540.0;
        let memory = matches!(self.metric, Metric::Memory);
        let lines: Vec<String> = (caption(self.row, self.claim).into_iter())
            .chain(omitted(self.claim, memory))
            .collect();
        let bottom = TOP + ROW * self.bars.len() as f64;
        let height = bottom + 36.0 + 20.0 * lines.len() as f64;
        let mut svg = Svg::new(height, theme);
        let unit: fn(f64) -> String = match self.metric {
            Metric::Speed => seconds,
            Metric::Memory => bytes,
        };
        let (tag, title, subtitle, mean) = match self.metric {
            Metric::Speed => (
                "SPEED",
                "Time to the same accuracy",
                "T(δ): time to the converged energy within δ, the median of timed runs · lower is better",
                "G",
            ),
            Metric::Memory => (
                "MEMORY",
                "Peak memory at the same accuracy",
                "peak Pss: the median of timed runs · lower is better",
                "R",
            ),
        };
        svg.head(tag, title, subtitle);
        svg.heroes(&self.heroes);
        svg.text(
            PAD,
            TOP - 36.0,
            14.0,
            " font-weight=\"600\"",
            PALETTE.muted,
            &format!("one representative cell: {}", self.input),
        );
        svg.text(
            PAD,
            TOP - 16.0,
            12.0,
            "",
            PALETTE.muted,
            &format!(
                "of the cells that every rival ran, the one whose ratios lie nearest each {mean}; ties go to the first by name"
            ),
        );
        let top = (self.bars.iter())
            .map(|b| b.2)
            .filter(|v| v.is_finite())
            .fold(0.0, f64::max);
        for (i, (label, fill, value)) in self.bars.iter().enumerate() {
            let y = TOP + ROW * i as f64;
            let length = match value.is_finite() && top > 0.0 {
                true => value / top * LENGTH,
                false => LENGTH,
            };
            svg.text(
                LEFT - 16.0,
                y + BAR / 2.0 + 6.0,
                17.0,
                " font-weight=\"600\" text-anchor=\"end\"",
                PALETTE.text,
                label,
            );
            svg.rect(LEFT, y, LENGTH, BAR, 6.0, PALETTE.track);
            svg.rect(LEFT, y, length, BAR, 6.0, *fill);
            svg.text(
                LEFT + length + 12.0,
                y + BAR / 2.0 + 6.0,
                17.0,
                " font-weight=\"600\"",
                PALETTE.text,
                &unit(*value),
            );
        }
        svg.caption(bottom + 24.0, &lines);
        svg.end()
    }
}

struct Scaling<'a> {
    row: &'a Row,
    claim: &'a Claim,
    heroes: Vec<Hero>,
}

fn domain(s: &Series) -> String {
    match (s.points.first(), s.points.last()) {
        (Some(a), Some(b)) => format!("{}–{} atoms", a.atoms, b.atoms),
        _ => "no point".into(),
    }
}

fn scaling(rows: &[Row]) -> Option<Scaling<'_>> {
    let (row, claim) = (rows.iter()).find_map(|r| {
        let claim = r.claim.as_ref()?;
        let ours = claim.series.iter().any(|s| s.code == ORBITAR);
        (ours && claim.series.len() > 1).then_some((r, claim))
    })?;
    let ours = claim.series.iter().find(|s| s.code == ORBITAR)?;
    let sizes: BTreeSet<u32> = (claim.series.iter())
        .flat_map(|s| s.points.iter().map(|p| p.atoms))
        .collect();
    if sizes.len() < 2 {
        return None;
    }
    let mut cells: Vec<(u32, &str)> = (ours.points.iter())
        .map(|p| (p.atoms, p.input.as_str()))
        .chain(
            (claim.skip.iter())
                .filter(|s| s.code == ORBITAR)
                .map(|s| (s.atoms, s.input.as_str())),
        )
        .collect();
    cells.sort_unstable();
    let largest = cells.last().map_or(0, |c| c.0);
    let at_largest: Vec<&str> = (cells.iter())
        .filter(|c| c.0 == largest)
        .map(|c| c.1)
        .collect();
    let mut heroes = Vec::new();
    let mut alphas = Vec::new();
    for s in claim.series.iter().filter(|s| s.code != ORBITAR) {
        let name = named(&s.code);
        alphas.push(format!(
            "{name} α {} over {}",
            exponent(s.t_delta_exponent),
            domain(s)
        ));
        let [input] = at_largest[..] else {
            heroes.push(Hero {
                big: "none".into(),
                power: String::new(),
                line: format!("no {name} ratio at {largest} atoms"),
                note: format!("{} cells share the largest size", at_largest.len()),
            });
            continue;
        };
        let point = |series: &Series| {
            series
                .points
                .iter()
                .find(|p| p.input == input)
                .map(|p| p.t_delta_s)
        };
        match (point(s), point(ours)) {
            (Some(theirs), Some(mine)) => {
                let ratio = theirs / mine;
                heroes.push(Hero {
                    big: format!("{}×", cut(ratio)),
                    power: String::new(),
                    line: match ratio > 1.0 {
                        true => format!("faster than {name} at {largest} atoms"),
                        false => format!("{name} time ÷ orbitaR time at {largest} atoms"),
                    },
                    note: format!("median T(δ) of {}", stem(input)),
                });
            }
            _ => {
                let why = (claim.skipped(&s.code, input))
                    .or_else(|| claim.skipped(ORBITAR, input))
                    .map_or("no point", |k| k.kind.name());
                heroes.push(Hero {
                    big: "none".into(),
                    power: String::new(),
                    line: format!("no {name} ratio at {largest} atoms"),
                    note: format!("{} {why}", stem(input)),
                });
            }
        }
    }
    heroes.push(Hero {
        big: "N".into(),
        power: exponent(ours.t_delta_exponent),
        line: "orbitaR time grows as N^α".into(),
        note: std::iter::once(format!("least squares over {}", domain(ours)))
            .chain(alphas)
            .collect::<Vec<_>>()
            .join(" · "),
    });
    Some(Scaling { row, claim, heroes })
}

fn exponent(alpha: f64) -> String {
    match alpha.is_finite() {
        true => format!("{alpha:.2}"),
        false => "none".into(),
    }
}

fn decade(k: f64) -> String {
    match k {
        k if k >= 0.0 => format!("1{} s", "0".repeat(k as usize)),
        k => format!("0.{}1 s", "0".repeat((-k - 1.0) as usize)),
    }
}

impl Card for Scaling<'_> {
    fn draw(&self, theme: usize) -> String {
        const TOP: f64 = 350.0;
        const PLOT: f64 = 300.0;
        const LEFT: f64 = PAD + 72.0;
        const RIGHT: f64 = WIDTH - PAD - 96.0;
        let lines: Vec<String> = (caption(self.row, self.claim).into_iter())
            .chain(omitted(self.claim, false))
            .collect();
        let bottom = TOP + PLOT;
        let height = bottom + 84.0 + 20.0 * lines.len() as f64;
        let mut svg = Svg::new(height, theme);
        svg.head(
            "SCALABILITY",
            "Time as cells grow",
            "median T(δ) per cell size · log–log axes · lower is better",
        );
        svg.heroes(&self.heroes);
        let points = || self.claim.series.iter().flat_map(|s| s.points.iter());
        let sizes: BTreeSet<u32> = points().map(|p| p.atoms).collect();
        let (first, last) = (
            f64::from(*sizes.first().unwrap_or(&1)),
            f64::from(*sizes.last().unwrap_or(&1)),
        );
        let (lo, hi) = (log10(first), log10(last));
        let times: Vec<f64> = points().map(|p| log10(p.t_delta_s)).collect();
        let low = times.iter().copied().fold(f64::INFINITY, f64::min).floor();
        let high = times
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil();
        let high = if high > low { high } else { low + 1.0 };
        let x = |atoms: u32| LEFT + (log10(f64::from(atoms)) - lo) / (hi - lo) * (RIGHT - LEFT);
        let y = |t: f64| bottom - (log10(t) - low) / (high - low) * PLOT;
        let mut k = low;
        while k <= high {
            let at = bottom - (k - low) / (high - low) * PLOT;
            svg.line((LEFT, at), (RIGHT, at), PALETTE.grid, 1.0);
            svg.text(
                LEFT - 12.0,
                at + 5.0,
                13.0,
                " text-anchor=\"end\"",
                PALETTE.muted,
                &decade(k),
            );
            k += 1.0;
        }
        for &atoms in &sizes {
            let at = x(atoms);
            svg.line((at, TOP), (at, bottom), PALETTE.grid, 1.0);
            svg.text(
                at,
                bottom + 22.0,
                13.0,
                " text-anchor=\"middle\"",
                PALETTE.muted,
                &atoms.to_string(),
            );
        }
        svg.text(
            (LEFT + RIGHT) / 2.0,
            bottom + 44.0,
            13.0,
            " text-anchor=\"middle\"",
            PALETTE.muted,
            "atoms per cell",
        );
        for s in &self.claim.series {
            let fill = colour(&s.code);
            let path: Vec<String> = (s.points.iter())
                .map(|p| format!("{},{}", px(x(p.atoms)), px(y(p.t_delta_s))))
                .collect();
            let _ = writeln!(
                svg.out,
                "<polyline points=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"3\" stroke-linejoin=\"round\"/>",
                path.join(" "),
                fill[theme]
            );
            for p in &s.points {
                let _ = writeln!(
                    svg.out,
                    "<circle cx=\"{}\" cy=\"{}\" r=\"5\" fill=\"{}\"/>",
                    px(x(p.atoms)),
                    px(y(p.t_delta_s)),
                    fill[theme]
                );
            }
            if let Some(p) = s.points.last() {
                svg.text(
                    x(p.atoms) + 12.0,
                    y(p.t_delta_s) + 5.0,
                    15.0,
                    " font-weight=\"600\"",
                    fill,
                    &named(&s.code),
                );
            }
        }
        svg.caption(bottom + 72.0, &lines);
        svg.end()
    }
}

enum Tile {
    Measured {
        case: u32,
        metric: &'static str,
        label: [&'static str; 2],
    },
    Derivatives {
        cases: &'static [u32],
        metrics: [&'static str; 2],
        label: [&'static str; 2],
    },
    Identical {
        case: u32,
    },
}

const TILES: [Tile; 4] = [
    Tile::Measured {
        case: 6,
        metric: "free_energy_error_ha",
        label: ["free energy vs ABINIT", "PBE, same datasets"],
    },
    Tile::Measured {
        case: 20,
        metric: "free_energy_error_ha",
        label: ["free energy vs ABINIT", "r2SCAN, same datasets"],
    },
    Tile::Derivatives {
        cases: &[13, 15, 22],
        metrics: ["_force_difference_ha_bohr", "_stress_difference_ha_bohr3"],
        label: ["forces and stress vs", "finite differences of F"],
    },
    Tile::Identical { case: 9 },
];

struct Drawn {
    big: String,
    label: [String; 2],
    note: String,
}

struct Verification {
    tiles: Vec<Drawn>,
    commits: Vec<String>,
}

fn verification(rows: &[Row]) -> Option<Verification> {
    let passed = |case: u32| (rows.iter()).find(|r| r.number == case && r.outcome == Outcome::Pass);
    let mut commits = BTreeSet::new();
    let mut tiles = Vec::new();
    for tile in &TILES {
        let drawn = match tile {
            Tile::Measured {
                case,
                metric,
                label,
            } => {
                let row = passed(*case)?;
                commits.extend(row.commit.clone());
                let m = row.metrics.iter().find(|m| m.name == *metric)?;
                Drawn {
                    big: format!("{} {}", scientific(m.measured), row.unit),
                    label: label.map(str::to_owned),
                    note: format!("case {case} · bound {}", scientific(m.limit)),
                }
            }
            Tile::Derivatives {
                cases,
                metrics,
                label,
            } => {
                let mut worst = None::<f64>;
                for case in *cases {
                    let row = passed(*case)?;
                    commits.extend(row.commit.clone());
                    for m in (row.metrics.iter())
                        .filter(|m| metrics.iter().any(|suffix| m.name.ends_with(suffix)))
                    {
                        worst = Some(worst.map_or(m.ratio, |w| w.max(m.ratio)));
                    }
                }
                let numbers: Vec<String> = cases.iter().map(u32::to_string).collect();
                Drawn {
                    big: "dF/dx ✓".into(),
                    label: label.map(str::to_owned),
                    note: format!(
                        "cases {} · worst ≤ {:.0}% of its bound",
                        numbers.join(", "),
                        (worst? * 100.0).ceil()
                    ),
                }
            }
            Tile::Identical { case } => {
                let row = passed(*case)?;
                commits.extend(row.commit.clone());
                let zero = |v: f64| v.to_bits() == 0.0_f64.to_bits();
                if !row
                    .metrics
                    .iter()
                    .all(|m| zero(m.measured) && zero(m.limit))
                {
                    return None;
                }
                let (first, second) = row.what.split_once(", ").unwrap_or((&row.what, ""));
                Drawn {
                    big: "0 bits".into(),
                    label: [first.to_owned(), second.to_owned()],
                    note: format!("case {case} · {} quantities, each 0", row.metrics.len()),
                }
            }
        };
        tiles.push(drawn);
    }
    let commits = commits.iter().map(|c| c[..7].to_owned()).collect();
    Some(Verification { tiles, commits })
}

impl Card for Verification {
    fn draw(&self, theme: usize) -> String {
        const TOP: f64 = 152.0;
        const TILE: f64 = 168.0;
        const GAP: f64 = 16.0;
        let height = TOP + TILE + 84.0;
        let mut svg = Svg::new(height, theme);
        svg.head(
            "VERIFICATION",
            "The engine computes what it claims",
            "each number from an active case's record, against the case's own bound",
        );
        let width =
            (WIDTH - 2.0 * PAD - GAP * (self.tiles.len() as f64 - 1.0)) / self.tiles.len() as f64;
        for (i, tile) in self.tiles.iter().enumerate() {
            let x = PAD + (width + GAP) * i as f64;
            let _ = writeln!(
                svg.out,
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"12\" fill=\"none\" stroke=\"{}\"/>",
                px(x),
                px(TOP),
                px(width),
                px(TILE),
                PALETTE.border[theme]
            );
            let x = x + 16.0;
            svg.text(
                x,
                TOP + 46.0,
                24.0,
                " font-weight=\"700\"",
                PALETTE.orbitar,
                &tile.big,
            );
            svg.text(x, TOP + 80.0, 14.0, "", PALETTE.text, &tile.label[0]);
            svg.text(x, TOP + 99.0, 14.0, "", PALETTE.text, &tile.label[1]);
            for (j, part) in tile.note.split(" · ").enumerate() {
                svg.text(
                    x,
                    TOP + 128.0 + 16.0 * j as f64,
                    12.0,
                    "",
                    PALETTE.muted,
                    part,
                );
            }
        }
        svg.caption(
            TOP + TILE + 44.0,
            &[format!(
                "orbitaR at commit {} · code verification, separate from accuracy",
                self.commits.join(", ")
            )],
        );
        svg.end()
    }
}
