# 0061. A blueprint is written at the lowest schema that holds it

**Status**: Accepted
**Date**: 2026-10-08

## Context

A blueprint leaves this repository: `mixengine-packages` signs and publishes every gallery manifest,
and installed builds of every age import those files. A build refuses a manifest whose `schema` is
higher than the one it reads (`UnknownBlueprintSchema`), and ignores keys it does not know.

T205 adds the first feature that needs a new schema (`[scaffold] archive`) and the first key that
does not (`[[next_steps]]`). T204a will add a third (several sites in one blueprint). Until now
`render` wrote the one schema the build knew, so raising it for one feature would have made every
file this build writes unreadable to every older build, including files that use nothing new.

## Decision

- **`render` writes the lowest schema that holds the manifest.** A file that uses nothing newer
  than schema 1 is written as schema 1, whatever the build that wrote it reads.
- **A key that changes what an apply does raises the schema. A key that only informs does not.**
  An older build that ignores an informative key applies the blueprint exactly as its author
  intended. One that ignored a behavioural key would apply something else, which is what the
  refusal exists to prevent.
- `SCHEMA` in `core::blueprints::manifest` is the highest schema this build **reads**. What it
  writes is `schema_of`'s answer.

## Consequences

- After T205 only `wordpress.toml` is schema 2. Every other gallery file stays readable by every
  installed build.
- T204a's several-site blueprints are schema 3, and only they are.
- An older build that imports a schema-1 file carrying `[[next_steps]]` stores its own rendering
  without them. The information is lost and the behaviour is not.
- A schema bump is no longer a release-wide event, so it needs no ADR of its own: the next one
  states which key raises it and why that key changes what an apply does.

Design: [2026-10-08-t205-a-blueprint-ends-at-a-working-site-design.md](../specs/2026-10-08-t205-a-blueprint-ends-at-a-working-site-design.md), D3.
