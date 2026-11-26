# ADR-0049: The DevOps IDE is DevopsDaruma

- **Status:** Accepted
- **Date:** 2026-10-18
- **Amends:** [ADR-0019](0019-alliterative-product-names.md) (adds a product)

## Context

The owner asked for an IDE for DevOps work in the same family and on the same platform
(2026-10-18): infrastructure as code, containers, pipelines and the files around them. Every
product name is two words with the same first letter (ADR-0019); like "Web", the first word may name
the field instead of a language.

## Decision

1. The DevOps IDE is **DevopsDaruma**: id `devopsdaruma`, bundle identifier
   `space.ifzenelse.devopsdaruma`, launcher `daruma`. A daruma is the Japanese doll that always
   stands back up ("fall down seven times, stand up eight"), the image of what DevOps is for:
   systems that recover and keep running. D-D alliterates. The owner chose it from four proposals.
2. Its icon is ours, drawn in the style of the Zen icon set: a white daruma on a pastel red tile,
   one eye painted and one still open, as a daruma waits for its goal.
3. It is built on the Zen Platform like every IDE; until its language module exists it is listed as
   "Coming soon".

## Consequences

- The product lists (Zen Toolbox, the website, README, naming, trademarks) include DevopsDaruma.
- What the IDE covers (Dockerfiles, Compose, Kubernetes, Terraform, CI files, shell) is decided when
  its work starts.
