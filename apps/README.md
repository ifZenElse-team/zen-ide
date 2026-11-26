# Applications

Every binary the project ships: the IDEs (`go-garden`, released; `rust-ronin`, in preview, ADR-0026;
`web-wave`, the web IDE, in preview; `jvm-jizo`, the IDE for Java and the JVM, in preview from 26.10.2; `sql-scroll`, the IDE for SQL and databases, and `devops-daruma`, the IDE for DevOps, both in preview from 26.11.8, ADR-0276 and ADR-0285), `zen-toolbox`, `zen-releases`, the release server and its publishing tool, `zen-pty-host`, which keeps the terminals running while an IDE restarts (ADR-0275), and `zen-cli`,
which lists and runs the platform's commands from the terminal. Directory and binary names are the
kebab-case product names (docs/naming.md).

An application is thin (ADR-0004, rule 5): branding, packaging metadata and the list of bundled
language modules. It contains no feature logic; features belong in platform crates or language
modules.
