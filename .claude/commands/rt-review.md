---
description: Adversarial red-zone review of realtime audio code — mechanical checks, then an independent critic pass
argument-hint: "[path, commit range, or blank for the working diff]"
allowed-tools: Read, Grep, Glob, Bash, Agent
---

Red-zone review of: **$ARGUMENTS**

If that is blank, review the uncommitted working diff. If it names a path,
review that file or directory. If it looks like a commit or range, review that
diff.

Run the phases in order. Do not skip the mechanical pass — it is free, and it
is more reliable than any amount of reading.

## Phase 1 — mechanical

These catch the obvious cases before an agent is spent on them.

    cargo clippy --all-targets 2>&1 | tail -30

Grep the red-zone surface for banned constructs. Adjust the path to whatever
the audio modules are actually called:

    rg -n 'unwrap\(|expect\(|panic!|format!|to_owned\(|to_string\(|\.collect\(|Box::new|Vec::|vec!|Mutex|RwLock|parking_lot|println!|eprintln!|dbg!|log::' \
       src/ -g '!src/bin/**' || echo "no banned constructs matched"

If dependencies changed, confirm the JACK backend is still really linked:

    nm -C target/debug/build/rtaudio-sys-*/out/build/librtaudio.a | grep -c RtApiJack
    # must be 24, not 0

Report each mechanical result as pass or fail with the actual output. A grep
hit is a candidate, not a verdict — Phase 2 decides.

## Phase 2 — independent critique

Hand the diff to the `rt-critic` subagent. Use the Agent tool with
`subagent_type: "rt-critic"`. Give it:

- the exact diff or file contents under review
- which functions are reachable from the audio callback, if known
- the Phase 1 grep hits, as candidates to confirm or refute

Independence is the point. Do not pre-filter its input with your own judgment
about what is fine, and do not argue with its findings on the basis of what the
code was *meant* to do.

## Phase 3 — report

Present surviving findings most-severe first, in the critic's format
(SEVERITY / RULE / WHERE / WHAT / TRIGGER / REFUTED?).

Then state plainly:

- what was checked, so the coverage shape is visible
- what was **not** checked and why — unresolved `dyn` calls, code you could not
  reach, paths outside the diff
- for each `fatal` finding, the one-line reason it breaks the deadline

Do not fix anything. Do not approve. This command produces a report; the
decision is the human's.
