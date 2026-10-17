# Neoffice fork markers

Map of **our** divergence from upstream (`storytold/craft-libs`) in this fork (`bvisible/craft-libs`,
branch `version-15`). Everything that can carry a comment carries a `//// Neoffice — …` marker in
place: `// //// Neoffice — …` in Rust (a bare `////` is an error for clippy), `# //// Neoffice — …`
in TOML and YAML, `<!-- //// Neoffice — … -->` in Markdown. This file holds what cannot.

At the next upstream merge, `grep -rn "////"` in the source plus this file give the complete
picture of what is ours and why.

## What the fork adds

Two crates, shared by every Neoffice editor (decision of 2026-10-13): `craft-nora`, the client core
of Nora, the Neoffice assistant (sans I/O: protocol, conversation state machine, document actions),
and `craft-nora-ui`, its egui panel. No upstream code was changed, apart from the two lines that
register them in the root `Cargo.toml` and the two lines that list them in `README.md`. They use
nothing else of this workspace, except upstream's `craft-ai-core`, which `craft-nora` reads behind its
`ai-core` feature (Nora's errors and turns in the vocabulary the Craft apps share for AI jobs, 17.10).

## Files that cannot carry a comment

| File | Why it is ours |
|---|---|
| `Cargo.lock` | Lists the two added crates (`craft-nora`, `craft-nora-ui`; `craft-nora` names `craft-ai-core` among its dependencies) and nothing else. On a merge conflict, take upstream's file and run `cargo check`: cargo writes our two entries back. |
