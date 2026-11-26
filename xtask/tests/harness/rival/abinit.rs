use super::*;

#[test]
fn abinit_runs_on_rank_zeros_own_wall_not_the_sum_over_its_ranks() {
    for (n, sum) in ["2.4", "0.3"].into_iter().enumerate() {
        let abinit = format!(
            "echo ' ABINIT 10.0.3'\nsleep 0.3\necho ' ETOT  1  -2.0    -2.000E+00 1.0E-04 1.0E+00'\n\
             echo \"{{SCF_istep: 1 , Vnl|psi>: 55 , wall_time: '  0.30 [s] '}} <<< TIME\"\n\
             echo ' Proc.   0 individual time (sec): cpu=          0.3  wall=          0.3'\n\
             echo '--- !FinalSummary'\necho 'overall_cpu_time:          {sum}'\necho 'overall_wall_time:          {sum}'\n\
             echo 'mpi_procs: {}'",
            if n == 0 { 8 } else { 1 }
        );
        assert_eq!(
            first_loss(&format!("rival-ranks-{n}"), PW, &abinit, "abinit"),
            "",
            "{sum}"
        );
    }
}
