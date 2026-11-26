# ADR-0224: Java and Kotlin test rules as plug-in declarations

- **Status:** Proposed
- **Date:** 2026-11-04
- **Relates to:** [ADR-0209](0209-test-naming-providers-and-test-history.md) (the `tests`
  declaration; extended here), [ADR-0050](0050-products-are-plugin-bundles.md) (plug-ins first),
  [ADR-0199](0199-project-structure-shows-the-model-of-every-language.md) (the project model)

## Context

ADR-0209 lets a plug-in declare Go to Test and Create Test as data. The Java and Kotlin plug-ins are
the first to use it. Three things did not fit the format as it was: a regular expression without
look-ahead cannot tell a Java method `int total(` from a statement `return total(`; Kotlin tests
are often named in backticks (``fun `total is zero`()``); and the Create Test dialog offered only
the frameworks a project uses, so a JUnit 4 project could not choose JUnit 5.

## Decision

1. **The Java and Kotlin plug-ins declare their rules under `"tests"`**: `Foo` pairs with `FooTest`,
   `FooTests`, `FooIT` and `TestFoo` in the test source folder of the same module and package, by
   the Maven and Gradle conventions (`src/main/java` with `src/test/java`; for Kotlin also
   `src/main/kotlin`, `src/commonMain/kotlin` and `src/jvmMain/kotlin` with their test folders).
   The module is the folder in front of these. Folders that a build moves elsewhere, which the
   project model of ADR-0199 knows, are not reachable from a declaration and are not followed yet.
2. **Frameworks are detected by the build files that name them** (`pom.xml`, `build.gradle`,
   `build.gradle.kts`, `gradle/libs.versions.toml` of the module or the project, by a dependency's
   coordinates): JUnit 5, JUnit 4 and TestNG, and kotlin.test for Kotlin. `detect` already expresses
   "depends on" well enough; no new field. The frameworks detected come first, then the others,
   in the order declared.
3. **The declaration gains `keywords`**: a match of `members` with one of these words as a whole
   named group is left out. A **name in backticks** is taken without them. Both are generic and
   optional, so other declarations are unchanged.
4. **The templates are our own**, written from the frameworks' public documentation: the package
   line, the imports, `setUp` and `tearDown` with the framework's annotations, and a `@Test` method
   per member, named after the member.

## Consequences

- JvmJizo and every IDE with the Java or Kotlin plug-in get Go to Test and Create Test with no code
  in the plug-ins; the plug-ins need an IDE released after 26.11.2, and an older one ignores the key.
- Detection by text can be fooled by a commented-out dependency; the build model of ADR-0195 can
  replace it later without changing the plug-ins.
- Members found by an expression are names only, without their class; a nested class's methods are
  listed with the outer class's.
