# ADR-0067: The QA IDE is TestTotami, a bundle of separate QA plug-ins

- **Status:** Accepted; decision 6 (optional plug-ins of a bundle) is implemented by [ADR-0073](0073-optional-bundle-plugins-areas.md)
- **Date:** 2026-10-19
- **Amends:** [ADR-0019](0019-alliterative-product-names.md) (adds a product)
- **Extends:** [ADR-0050](0050-products-are-plugin-bundles.md) (a bundle's optional plug-ins)

## Context

The owner asked for one more product in the family (2026-10-19): an IDE for quality
assurance, covering "all of it": automated end-to-end and UI tests, API testing, load testing and
test management. Two further requirements of the owner settle its shape: TestTotami ships every QA
plug-in, and the user simply switches on what they need; and it is **modular**, several separate
plug-ins that are switched on and off one by one, not one large plug-in, so that the IDE loads only
what the user works with.

ADR-0050 makes every capability a plug-in for every IDE and a product a bundle whose plug-ins are
"all installed and on from the first start". A QA engineer who only automates API checks should not
pay for a browser-test runner and a load-test dashboard they never open, so that rule needs an
amendment for TestTotami.

A plug-in today is a WebAssembly module without imports (ADR-0031, ADR-0056): it highlights, folds,
outlines and completes, but it cannot start a process, reach the network or show a tool window. The
HTTP Client (ADR-0061) set the pattern for what a plug-in cannot do: the language is a plug-in, the
running is the platform's.

## Decision

1. **The QA IDE is TestTotami**: id `testtotami`, bundle identifier
   `space.ifzenelse.testtotami`, launcher `totami`, app crate `apps/test-totami` when it is built.
   As "Web" and "Devops" do, the first word names the field, **Test**, and T-T alliterates. The
   image is the tatami, the woven straw mat of a Japanese room, spelled **Totami** as the owner
   named it: a tatami floor is laid mat by mat in a fixed pattern until no square is left
   uncovered, which is what a test suite does to a product.
2. **Its audience** is QA engineers and test automation engineers (SDETs), QA leads who keep the
   test plans, and developers who write and run the tests of their own code.
3. **Its icon is ours**, in the style of the Zen icon set: on a straw-coloured tile (`#DCCB86`, the
   colour of a new tatami mat) four black mats laid in the pinwheel of a four-and-a-half-mat room,
   with a check mark on the half mat in the middle. Its two code colours (Zen Toolbox's drawn tile)
   are `#E6D38C` and `#9A862E`.
4. **The bundle** is the base set (ADR-0036, ADR-0050; it already holds the `http` plug-in of the
   HTTP Client) plus four separate QA plug-ins. Each works in every Zen IDE (ADR-0050 decision 1)
   and is in the catalog for the others:

   | Plug-in | id | What it does |
   |---|---|---|
   | E2E and UI tests | `e2e-tests` | Recognises Playwright, Cypress and WebdriverIO specs (on the base set's TypeScript and JavaScript): run icons in the gutter for a test, a file and a suite; run configurations that start the project's own runner (ADR-0048); results in the Run tool window's test tree, with the failure's screenshot, video and trace opened from it; the tests in the Structure view; completion of the runner's API. Playwright first. |
   | API test collections | `api-tests` | Grows the HTTP Client (ADR-0061) into test collections: assertions and response checks in `.http` files, a collection (a folder or a file) run as one with a pass and fail report, environments per stage, chaining a response into the next request; later, importing Postman and Insomnia collections and OpenAPI descriptions into `.http` files. Requests are sent by the platform exactly as ADR-0061 sends them. |
   | Load testing (k6) | `load-tests` | k6 scripts: completion and documentation of the `k6/*` modules, options, checks, thresholds and scenarios; a run configuration that starts the user's own `k6` (found on `PATH`, never downloaded without asking); a live summary of requests, latencies, checks and thresholds in the Run tool window. The id leaves room for other load-test tools later; k6 comes first. |
   | Test management | `test-management` | Test cases as files in the repository (Markdown with a small front matter: id, title, steps, expected result, priority, tags): a Test Cases tool window with suites and cases, manual test runs whose results (passed, failed, blocked, skipped) are written to files, links from a case to the automated tests that cover it and to the task it checks, and a coverage report. Offline: no server; integrations with external test management systems are later plug-ins, each with explicit network access. |

5. **Separate and switchable; off means not loaded.** TestTotami ships all four in its bundle, so
   that nothing has to be downloaded, but each is switched on and off on its own in Settings →
   Plugins → Installed (Bundled with this IDE), and a plug-in that is off is not loaded at all,
   as the IDE already does for any switched-off plug-in. The four never depend on each other; a
   link between them (a test case pointing at an E2E test) is a plain reference that is shown when
   both are on and ignored otherwise.
6. **Optional plug-ins of a bundle** (amends ADR-0050 decision 2 for products that need it): a
   product's bundle may mark some of its plug-ins as **areas**, shipped but off until chosen. On its
   first start TestTotami asks which areas the user works in (E2E and UI tests, API tests, load
   testing, test management; any number, all of them allowed), switches on those and leaves the
   others off; the choice can be changed at any time in Settings → Plugins. The choice is a command
   with typed parameters, reachable over MCP like every other (ADR-0033), and it is a platform
   feature, not TestTotami's: any product's bundle can declare areas. The base set is never an
   area: it is always on.
7. **What a plug-in cannot do, the platform does**, as for the HTTP Client: the plug-ins declare
   and recognise (test files, run targets, k6 modules, test case files); starting runners and
   `k6`, the test tree, sending requests and the tool windows are shared platform services that
   every IDE gets. Where the plug-in ABI or the run service lacks something a QA plug-in needs, it
   grows in the platform for every plug-in (ADR-0050 decision 1), never inside one plug-in.
8. **Nothing runs or connects by itself** (AGENTS.md, rule 10): a test, a collection or a load test
   starts only when the user or an agent runs it; no runner, browser or `k6` is downloaded without
   asking.
9. **The order of work:**
   1. The product (this ADR): name, icon, product lists, the release site's entry.
   2. **API test collections** (`api-tests`): the smallest step, on the HTTP Client and
      the `http` plug-in that already exist.
   3. **E2E and UI tests** (`e2e-tests`): the largest audience; Playwright first.
   4. **Optional plug-ins of a bundle and the first-start choice of areas** (decision 6, a platform
      task): needed before TestTotami is built as an app.
   5. **The TestTotami app**: `apps/test-totami`, its entry in `IDES` and `PRODUCT_PLUGINS`
      (`xtask/src/release.rs`) with the plug-ins written so far, and its first release when the
      owner asks.
   6. **Test management** (`test-management`).
   7. **Load testing** (`load-tests`), k6 first.

## Consequences

- The product lists (Zen Toolbox, the website, README, naming, trademarks, NOTICE)
  include TestTotami; Zen Toolbox lists it as "Coming soon" and the website as "Planned".
- Until its app exists, TestTotami's bundle is declared in prose next to the other IDEs that are
  not apps yet ([docs/plugins.md](../plugins.md#in-the-ide)), as DevopsDaruma's is:
  `PRODUCT_PLUGINS` accepts only products listed in `IDES`, which are the IDEs that can be
  released. Its entry there is `("testtotami", &["api-tests", "e2e-tests", "test-management",
  "load-tests"])`, with only the plug-ins that exist at the time.
- The QA plug-ins are planned as TestTotami's tasks (ADR-0050, consequence 1), each with "works in
  every IDE" and "off means not loaded" in its acceptance criteria. Two tasks that are not plug-ins
  are needed besides them: the optional plug-ins and the first-start choice of areas (the
  platform), and the TestTotami app.

## Alternatives considered

- **One QA plug-in with everything**: the owner rejected it ("modular, so as not to overload"); it
  would load a browser-test runner for someone who only tests APIs.
- **Only the base set, QA plug-ins from the catalog**: TestTotami would then not be a ready-made QA
  IDE (ADR-0050 decision 2), and it would need the network on the first start.
- **All four on from the first start, as ADR-0050 says for other products**: the user would have to
  find and switch off what they do not use; the choice of areas does it once, up front.
