use xtask::harness::{Evidence, Metrics};

const PAIRS: [(&str, &str, bool, usize); 3] = [
    ("input", "fe-full", true, 4),
    ("ge", "ge-full", false, 8),
    ("si-r2scan", "si-r2scan-full", false, 8),
];

pub fn measure(evidence: &Evidence) -> Result<Metrics, String> {
    crate::n7_symmetry::pairs(evidence, &PAIRS)
}
