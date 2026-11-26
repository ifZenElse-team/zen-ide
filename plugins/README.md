# Plug-ins

Each folder is one plug-in: a crate named `zen-plugin-<id>` built as a WebAssembly module, and a
`plugin.json` with its display name, description and file extensions. Plug-ins depend on no
workspace crate; they talk to the IDE only through the plug-in ABI: v0 highlights, v1 adds folding,
an outline, completion and abbreviation expansion ([ADR-0056](../docs/adr/0056-plugin-abi-v1.md)),
colour literals and what typing adds ([ADR-0063](../docs/adr/0063-plugin-colours-and-typing.md)).
Everything else, including how to publish one, is in [docs/plugins.md](../docs/plugins.md) and
[ADR-0031](../docs/adr/0031-plugins.md).

| Plug-in | Supports | Bundled with |
|---|---|---|
| [markdown](markdown) | `*.md`, `*.markdown`: highlighting, the headings in the Structure view, folding of sections and fenced code (the preview is the IDE's own, ADR-0138) | every IDE (base set) |
| [yaml](yaml) | `*.yml`, `*.yaml`, including Kubernetes manifests (Compose files too, unless the Docker plug-in takes them by name) | every IDE (base set) |
| [json](json) | `*.json`, `*.jsonc`, `*.json5`, `*.jsonl`, `*.ndjson`, `*.geojson`, `*.webmanifest`, `*.har`, `.babelrc`, `.eslintrc`, `.prettierrc`, `.jshintrc`, `composer.lock` | every IDE (base set) |
| [toml](toml) | `*.toml`, `Cargo.lock`, `Pipfile`, `poetry.lock`, `uv.lock` | every IDE (base set) |
| [dockerfile](dockerfile) ("Docker") | `Dockerfile`, `Dockerfile.*`, `*.Dockerfile`, `Containerfile`, `Containerfile.*`, `*.dockerfile`, `*.containerfile`; Docker Compose files by name: `docker-compose.yml`, `docker-compose.*.yml`, `compose.yml`, `compose.*.yml` (and `.yaml`) | every IDE (base set) |
| [gitlab-ci](gitlab-ci) | `.gitlab-ci.yml`, `.gitlab-ci.yaml`, `*.gitlab-ci.yml`, `*.gitlab-ci.yaml` (by file name) | every IDE (base set) |
| [ignore](ignore) | `.gitignore`, `.dockerignore`, `.npmignore`, `.ignore`, `.hgignore`, `.prettierignore`, `.eslintignore`, `.helmignore`, `.gcloudignore`, `.vercelignore`, `.stylelintignore`, `.cfignore` | every IDE (base set) |
| [env](env) | `*.env`, `.env`, `.env.*`, `.envrc`, `.flaskenv` | every IDE (base set) |
| [shell](shell) | `*.sh`, `*.bash`, `*.zsh`, `*.ksh`, `*.command`, `.bashrc`, `.bash_profile`, `.zshrc`, `.zprofile`, `.profile`, `.bash_aliases`, `*.bashrc` | every IDE (base set) |
| [xml](xml) | `*.xml`, `*.plist`, `*.xsd`, `*.xsl`, `*.xslt`, `*.svg`, `*.wsdl`, `*.pom`, `*.csproj`, `*.fsproj`, `*.vbproj`, `*.props`, `*.targets`, `*.resx`, `*.xaml`, `*.rss`, `*.atom`, `*.kml`, `*.gpx`, `*.opml`, `*.dtd`, `*.iml`; any file that starts with `<?xml`, a doctype other than HTML's, or a root element with `xmlns` | every IDE (base set) |
| [html](html) | `*.html`, `*.htm`, `*.xhtml` | every IDE (base set) |
| [css](css) | `*.css` | every IDE (base set) |
| [javascript](javascript) | `*.js`, `*.mjs`, `*.cjs`, `*.jsx` | every IDE (base set) |
| [typescript](typescript) | `*.ts`, `*.mts`, `*.cts`, `*.tsx` | every IDE (base set) |
| [scss](scss) | `*.scss` | WebWave; from the catalog in every other IDE |
| [vue](vue) | `*.vue` | from the catalog in every IDE, offered when a `.vue` file opens |
| [svelte](svelte) | `*.svelte` | from the catalog in every IDE, offered when a `.svelte` file opens |
| [java](java) | `*.java`: highlighting, folding, the Structure view, completion, file templates and the Java type of New Project in Rust; the rest through Eclipse JDT LS, downloaded into the IDE's folder on request (ADR-0148) | JvmJizo; from the catalog in every other IDE, offered when a `.java` file opens |
| [eslint](eslint) | ESLint projects (`eslint.config.*`, `.eslintrc*`, `eslint` in `package.json`): no file types; the ESLint server alongside script, Vue and Svelte files | from the catalog in every IDE, offered when an ESLint project opens |
| [prettier](prettier) | Prettier projects (`.prettierrc*`, `prettier.config.*`, `prettier` in `package.json`): no file types; the project's Prettier as formatter | from the catalog in every IDE, offered when a Prettier project opens |
| [stylelint](stylelint) | Stylelint projects (`.stylelintrc*`, `stylelint.config.*`, `stylelint` in `package.json`): no file types; the project's Stylelint runs `--fix` on save | from the catalog in every IDE, offered when a Stylelint project opens |
| [biome](biome) | Biome projects (`biome.json`, `biome.jsonc`): no file types; the project's Biome alongside, and formatting | from the catalog in every IDE, offered when a Biome project opens |
| [npm](npm) | Projects with a `package.json`: no file types; the npm tool window (scripts, install, offline mode). Run icons in `package.json` and External Libraries are in every IDE without it | WebWave; from the catalog in every other IDE, offered when a project with a `package.json` opens |
| [nodejs](nodejs) ("Node.js") | Projects with a `package.json`: no file types; Node.js run and debug configurations, Jest and Vitest with the test tree | WebWave; from the catalog in every other IDE, offered when a project with a `package.json` opens |
| [react](react) | React projects (`react` in `package.json`): no file types; `vite` run configurations in Vite projects, file templates | from the catalog in every IDE, offered when a React project opens |
| [next](next) | Next.js projects (`next.config.*`, or `next` in `package.json`): no file types, requires `react`; run configurations, App and Pages Router file templates | from the catalog in every IDE, offered when a Next.js project opens |
| [sveltekit](sveltekit) | SvelteKit projects (`@sveltejs/kit` in `package.json`): no file types, requires `svelte`; `vite` run configurations, `svelte-kit sync`, route file templates | from the catalog in every IDE, offered when a SvelteKit project opens |
| [nuxt](nuxt) | Nuxt projects (`nuxt.config.*`, or `nuxt` in `package.json`): no file types, requires `vue`; run configurations, `nuxi prepare`, file templates | from the catalog in every IDE, offered when a Nuxt project opens |
| [maven](maven) | Maven projects (`pom.xml`): takes `pom.xml` by name, read as XML (the XML plug-in's code) with POM completion; the Maven Archetype project type and modules in `<modules>`; the IDE's Maven tool window shows while it is on | JvmJizo; from the catalog in every other IDE, offered when a Maven project opens |
| [gradle](gradle) | Gradle builds (`settings.gradle(.kts)`, `build.gradle(.kts)`): `*.gradle` (Groovy build scripts) highlighted, folded and outlined; modules with `include`; the IDE's Gradle tool window shows while it is on | JvmJizo; from the catalog in every other IDE, offered when a Gradle build opens |
| [properties](properties) | `*.properties`: highlighting, folding, the keys in the Structure view, duplicate keys and malformed `\u` escapes, the unescaped value as an inlay (`zen_inlays`); keys and bundles named in Java and Kotlin string literals (`zen_key_references`); the platform's Resource Bundle editor groups `name_<locale>.properties` | JvmJizo; from the catalog in every other IDE, offered when a `.properties` file opens |
| [spring-boot](spring-boot) ("Spring Boot") | Spring Boot projects (`src/main/resources/application.properties`, `.yml` or `.yaml`): takes `application.properties`, `application-*.properties`, `application.yml`, `application-*.yml` (and `.yaml`) by name, read as properties (the Properties plug-in's code) or as YAML (the YAML highlighter) by their text; the platform completes, documents and checks their keys from the configuration metadata in the project's jars runs `@SpringBootApplication` classes as Spring Boot configurations, marks beans and injection points in the gutter, lists request mappings in the Endpoints tool window, marks Boot versions past their open-source support ([ADR-0217](../docs/adr/0217-spring-boot-plugin-metadata-from-the-projects-jars.md)) and adds the Spring Boot generator to New Project, from the Spring Initializr service ([ADR-0240](../docs/adr/0240-generators-with-a-service.md)) | JvmJizo; from the catalog in every other IDE, offered when a Spring Boot project opens |
| [kotlin](kotlin) | `*.kt`, `*.kts` (Gradle's Kotlin DSL among them): our own Kotlin lexer, folding, Structure; two servers to choose from, the Kotlin team's kotlin-lsp (default) and fwcd's, installed from archives on the user's word (ADR-0151); Kotlin file templates; the Kotlin type of New Project | none; from the catalog in every IDE, offered when a Kotlin file opens |
| [sql](sql) | `*.sql`, `*.ddl`, `*.dml`, `*.pgsql`, `*.psql`, `*.mysql`, `*.plsql` | none, from the catalog in every IDE |
| [sqlserver](sqlserver) ("SQL Server") | no file types; the database driver `sqlserver` (SQL Server and Azure SQL), served by the program `zen-db-sqlserver` from [drivers/sqlserver](../drivers/sqlserver) (ADR-0175); not released until driver programs can be published | none; from the catalog in every IDE once released |
| [mongodb](mongodb) ("MongoDB") | no file types; the database driver `mongodb` (MongoDB), served by the program `zen-db-mongodb` from [drivers/mongodb](../drivers/mongodb) (ADR-0175); not released until driver programs can be published | none; from the catalog in every IDE once released |
| [duckdb](duckdb) ("DuckDB") | no file types; the database driver `duckdb` (DuckDB files and in-memory databases), served by the program `zen-db-duckdb` from [drivers/duckdb](../drivers/duckdb), which drives the `duckdb` program the user installs (ADR-0175); not released until driver programs can be published | none; from the catalog in every IDE once released |
| [oracle](oracle) ("Oracle") | no file types; the database driver `oracle` (Oracle Database), served by the program `zen-db-oracle` from [drivers/oracle](../drivers/oracle), which drives SQLcl, the command-line program the user installs (ADR-0190); not released until driver programs can be published | none; from the catalog in every IDE once released |

The base set ships inside every IDE ([ADR-0036](../docs/adr/0036-bundled-plugins.md)), and each IDE
adds the plug-ins of its specialisation ([ADR-0050](../docs/adr/0050-products-are-plugin-bundles.md));
both lists are in `xtask/src/release.rs`, `BASE_PLUGINS` and `PRODUCT_PLUGINS` (GoGarden and
RustRonin add none for now, WebWave adds `scss`). A plug-in bundled with one IDE installs from the catalog in the others. Every release of a plug-in has its notes in
`changelog/plugins/<id>.md`.

A plug-in may share a source file with another one, since it cannot depend on it: the Docker
plug-in includes the YAML highlighter, `yaml/src/yaml.rs`, with
`#[path = "../../yaml/src/yaml.rs"] mod yaml;`. That file stays self-contained (no `crate::` paths,
no ABI code); a change to it changes both plug-ins, so test and release both.

The Maven plug-in includes the XML plug-in's code (`xml/src/xml.rs` and the HTML tokenizer it reads
with) so that `pom.xml`, which it takes by name, keeps everything XML gives it; a change to those
files changes the Maven plug-in too.

The Properties plug-in keeps its reader in one self-contained file, `properties/src/properties.rs`,
for a plug-in that takes some `.properties` files by name to include, as Maven includes the XML
plug-in's code: a framework plug-in declares the names (`"names": ["application.properties",
"application-*.properties"]`), includes the file with
`#[path = "../../properties/src/properties.rs"] mod properties;` and exports what the Properties
plug-in exports; a plug-in matched by name wins over one matched by extension, so such files get
both. It may answer `zen_complete` with `properties::complete_keys(text, caret, &KEYS)`, its own
keys offered where a key is typed (ADR-0214). The Spring Boot plug-in does not: the keys of its
files are the platform's, read from the configuration metadata in the project's jars
([ADR-0217](../docs/adr/0217-spring-boot-plugin-metadata-from-the-projects-jars.md)). It includes the
YAML highlighter too and reads a file as YAML or as properties by its text (`is_yaml`), so one module
serves `application.properties` and `application.yml`.

The SCSS plug-in is the CSS plug-in's code read as SCSS: it includes every module of `css/src`
(`#[path = "../../css/src/token.rs"] mod token;` and so on) and sets
`pub(crate) fn dialect() -> token::Dialect { token::Dialect::Scss }` at its crate root, where the CSS
plug-in answers `Dialect::Css`; the modules ask `token::scss()` for `//` comments, `$variables`,
`#{…}` interpolation, placeholders, mixins and the Sass at-rules. The HTML plug-in includes
`css/src/color.rs`, the self-contained parser and scanner of colour literals, for its `style`
attributes and `<style>` elements.

The TypeScript plug-in includes the JavaScript analyser the same way,
`#[path = "../../javascript/src/script.rs"] mod script;`, and picks the dialect with
`Dialect::TypeScript`. The analyser sees only the text, never the file name, so it tells JSX from
TypeScript's `<T>` generics and `<Type>value` casts by what follows the tag name: `<T>(`,
`<T,>`, `<T extends …>` and `<string>` are TypeScript; any other tag counts as JSX only when its
closing tag follows somewhere in the text, or, with attributes, a `/>`. Type positions are found without a type
checker: after `:` in parameters, return types, `let`/`const` bindings and class or interface
members, and after `extends`, `implements`, `as`, `satisfies`, `type X =` and generic arguments
of calls and `new`.

The same analyser answers typing (`jsx_on_type`, exported by both plug-ins as `zen_on_type` with the
trigger `>`): the end tag of a JSX start tag just closed, judged from the text by where its `<` stands, with
`<T>`, `<T,>`, `<T extends …>` and TypeScript's type names (`<string>`) taken for generics or casts
([ADR-0131](../docs/adr/0131-react-and-next-plugins.md)). The Vue plug-in includes it but does not
export it.

The Vue plug-in is built from the others' code: it includes the HTML tokenizer
(`html/src/lexer.rs`) for the blocks and the template, the JavaScript analyser
(`javascript/src/script.rs`) for the script and the template's expressions, and the CSS modules
(`token`, `tree`, `data`, `highlight`, `structure`, `complete`, `color`) for the style. Its
`dialect()` reads a thread-local set to the `lang` of the `<style>` block being read
(`with_style_dialect`), so the same CSS code reads CSS in one block and SCSS in the next. A change
to any of those files changes the Vue plug-in too: test and release it with them.

The Nuxt, React, Next.js and SvelteKit plug-ins' modules highlight nothing: what each gives is declared in its
`plugin.json` (`requires`, `project` and `fileTemplates`,
[ADR-0129](../docs/adr/0129-project-plugins-npm-run-and-nuxt.md), [ADR-0131](../docs/adr/0131-react-and-next-plugins.md))
and done by the IDE.

The Svelte plug-in reuses the same files as the Vue plug-in and the Vue plug-in's block reader
(`vue/src/sfc.rs`), and reads the markup itself (`svelte/src/markup.rs`): it hides the inside of
every `{…}` from the HTML tokenizer, keeping the offsets, so that an arrow in an attribute's
expression does not end the tag ([ADR-0132](../docs/adr/0132-svelte-plugin-and-language-server.md)).

The XML plug-in reads XML with the HTML tokenizer in its XML mode (`lex_xml` in `html/src/lexer.rs`:
no raw text elements, processing instructions end at `?>`, a doctype keeps its internal subset),
included with `#[path = "../../html/src/lexer.rs"] mod lexer;` as Vue and Svelte include it; names
are compared with their case and no element is void. A change to the lexer changes the HTML, Vue,
Svelte and XML plug-ins: test and release them together.
