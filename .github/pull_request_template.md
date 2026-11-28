<!-- Give one sentence. Skip per-file lists, diff summaries, and emoji. -->
Intent: [what result changes and why]

<!-- Include only for engine or public-number changes. -->
Owner reads: [file:fn] -> [file:fn]

<!-- Use at most 5 lines in the paper's symbols or pseudocode. Omit if unchanged, or for refactors, tools, docs, or diffs under 50 lines. -->
Core change:

```diff
- [previous equation, rule, or loop order]
+ [new equation, rule, or loop order]
```

<!-- Give measured evidence. Skip claims such as "robust" or "fully compatible". -->
Proof: [command] -> [metric] [old] -> [new] ([bound])

<!-- Say which checks you could not run, or write "nothing". -->
Not tested: [checks or nothing]

<!-- Name the file and values for each changed bound, or write "none". -->
Bars moved: [none or file: old -> new]

<!-- Omit unless you want feedback on a specific decision. Name just one. -->
Feedback wanted: [decision you are least sure of]

<!-- Record the review rounds, verdict and commit SHA, and LOCK verdict. -->
Review: [rounds, verdict at sha, LOCK AGREE/DISAGREE]

<!-- Give the commit SHA and any other steps needed to undo the change. -->
Rollback: [revert sha and anything else to undo]
