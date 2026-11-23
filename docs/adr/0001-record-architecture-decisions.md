# 0001. Record architecture decisions

- Status: Accepted
- Date: 2026-11-15

## Context

NemesisCommander is open source and was started from a written architecture
(v0.1). Many smaller decisions are taken while coding, often without the
project owner present. Contributors need to know *why* the code looks the
way it does.

## Decision

Every decision that affects more than one crate, deviates from the
architecture document, or would surprise a new contributor is recorded as a
short ADR in `docs/adr/`, numbered sequentially.

## Consequences

Reviewers can accept or reject a decision by editing a single file. A
decision that is reversed gets a new ADR that supersedes the old one.
