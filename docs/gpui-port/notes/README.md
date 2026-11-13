# Working notes of the GPUI rewrite

These files are the working state behind the code on this branch, kept here so it survives the session that wrote it.

- `backlog.md`: the queue of integrations, with the state after each one and the cautions for the next.
- `integration/`: step-by-step notes for wiring that is committed but not yet part of the build (files no `mod` line declares). Each was written without a compiler: every statement in them is a hypothesis until the code compiles and its tests pass.
- `reports/`: what the agents that wrote or integrated each slice reported.
- `wiring_brief.md`, `view_rules.md`: the rules those agents work under.
- `OWED.md`: what the final hand-over must tell the owner.
