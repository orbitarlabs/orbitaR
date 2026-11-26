#!/bin/sh
# The ABINIT 10.0.3 runs behind each <name>.json of a same-dataset case: <name>.abi on the full k mesh without symmetry
# (kptopt 3, nsym 1), so that ABINIT discretizes as the engine does, with every band converged. Before any run, each
# JTH file must match its sha256 in datasets/jth-2.0.toml ([pbe.element]: JTH-PBE-v2.0, the layout's directory).
# usage: sh run.sh <abinit binary> <JTH-PBE-v2.0 directory> <work directory>
#                  [<abi directory> <nstep> <mpiexec> <name>...]
#   case 6:   sh run.sh abinit JTH work                       its own .abi files, nstep 100, one rank
#   case 11:  sh run.sh abinit JTH work <case 11 reference> 200 mpiexec gaas al2o3 si32 nio
#             one MPI rank per k-point and spin: ABINIT 10.0.3 runs more with a warning ("This is a WASTE")
#        then python -I reference.py <work directory>/<name> > <name>.json for each name
set -e
ABINIT=$1; JTH=$2; WORK=$3
HERE=$(cd "$(dirname "$0")" && pwd)
DIR=${4:-$HERE}; NSTEP=${5:-100}; MPIEXEC=${6:-}
if [ $# -gt 6 ]; then shift 6; else set -- input al gaas fine fe afm; fi
PINS="$HERE/../../../datasets/jth-2.0.toml"
for name in "$@"; do
  for file in $(sed -n 's/^pseudos "\(.*\)"/\1/p' "$DIR/$name.abi" | tr -d ' ' | tr ',' ' '); do
    want=$(awk -v el="${file%%/*}" '/^\[/ { s = $0 } s == "[pbe.element]" && $1 == el { split($0, q, "\""); print q[2] }' "$PINS")
    echo "$want  $JTH/$file" | sha256sum -c - > /dev/null || { echo "$file: not the pinned JTH file" >&2; exit 1; }
  done
done
COMMON="kptopt 3
nsym 1
chkprim 0
chksymbreak 0
tolvrs 1e-24
tolwfr 1e-22
nline 8
nstep $NSTEP
prteig 0
iomode 3
pp_dirpath \"$JTH\""
for name in "$@"; do
  mkdir -p "$WORK/$name"
  { cat "$DIR/$name.abi"; printf '%s\n' "$COMMON"; } > "$WORK/$name/run.abi"
  if [ -n "$MPIEXEC" ]; then
    ranks=$(awk '$1 == "ngkpt" { k = $2 * $3 * $4 } $1 == "nsppol" { s = $2 } END { print k * (s ? s : 1) }' \
      "$DIR/$name.abi")
    (cd "$WORK/$name" && OMP_NUM_THREADS=1 "$MPIEXEC" -n "$ranks" "$ABINIT" run.abi > log 2>&1)
  else
    (cd "$WORK/$name" && OMP_NUM_THREADS=1 "$ABINIT" run.abi > log 2>&1)
  fi
done
