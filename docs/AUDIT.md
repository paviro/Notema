# Dependency audit

CI runs `cargo audit --deny warnings`, and so can you locally — acknowledged
advisories live in `.cargo/audit.toml`, so the command reproduces the CI result
without extra flags. Exceptions must name the transitive owner, state why the
code is not directly replaceable here, and carry a review date.

No acknowledged advisories.
