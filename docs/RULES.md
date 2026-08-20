# RULES.md

Universal rules that apply to every change in this repo. Concept and
architecture background: [CONCEPT.md](CONCEPT.md). What a new feature must
look like: [FEATURES.md](FEATURES.md). PR hygiene:
[CONTRIBUTING.md](../CONTRIBUTING.md).

## Sections

- [Smallest viable change](#smallest-viable-change)
- [No silent fallback](#no-silent-fallback)
- [Comments](#comments)
- [Formatting](#formatting)
- [Commits](#commits)
- [When in doubt](#when-in-doubt)

## Smallest viable change

- Long specs are upper bounds — slice them.
- Touch only the lines the change requires.
- No drive-by renames, import reordering, or refactors of code the change
  does not otherwise touch. Separate PR.
- If the same UI block or helper appears twice, factor it out before opening
  the PR.

## No silent fallback

One deterministic path per decision. Surface failures — never substitute a
different device, rate, or format behind the user's back. A backend that
cannot support the feature returns an error; it does not quietly fall back.

## Comments

- Comments only for non-obvious WHY (hidden constraint, invariant, workaround).
  Naming handles WHAT. Terse, one line. Section dividers only in files
    > 500 lines.
- Comments describe the code as it stands, never the edit or the conversation.
  No "now / instead of / previously / was", no narrating a change to the
  reviewer.

## Formatting

- Enforced: `bun run format` (Prettier + rustfmt) before every PR.
- The tree is clean under both, so it produces no churn — it only normalises
  the lines you wrote.
- Format-on-save is safe here and encouraged. Do not hand-format against the
  tool.

## Commits

```
type(scope): subject
```

[Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/).
Lowercase, no trailing period, body usually omitted. Types in use: `feat`,
`fix`, `chore`, `refactor`, `style`, `docs`. Keep formatting-only commits
separate from behavioural ones so they can be reviewed at a glance and
skipped in `git blame`.

## When in doubt

- Read the current code, not earlier explanations.
- RT path change → `cargo check`.
- Svelte change → `bun run check`.
- Rust `#[derive(TS)]` change → `bun run generate`, commit the generated
  files with the Rust change.
