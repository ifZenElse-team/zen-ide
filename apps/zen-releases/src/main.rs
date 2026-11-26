#![allow(clippy::print_stdout, clippy::print_stderr, reason = "a command-line tool reports to the terminal")]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "zen-releases", version, about = "The Zen IDE release server and publishing tool (ADR-0020).")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Args)]
struct PublishPlugin {
    /// The server's base URL.
    #[arg(long, env = "ZEN_RELEASES_SERVER", default_value = "http://127.0.0.1:8787")]
    server: String,
    /// The publisher's secret key file; without it, the key is read from ZEN_PUBLISHER_KEY (its
    /// hex, as a release pipeline's protected variable gives it, ADR-0171).
    #[arg(long)]
    key: Option<PathBuf>,
    /// The plug-in id, e.g. markdown.
    #[arg(long)]
    id: String,
    /// The display name, e.g. Markdown.
    #[arg(long)]
    name: String,
    /// One sentence about what it does.
    #[arg(long, default_value = "")]
    description: String,
    /// The file extensions it supports, comma-separated, without dots, e.g. md,markdown.
    #[arg(long, value_delimiter = ',')]
    extensions: Vec<String>,
    /// File name patterns it supports besides extensions, comma-separated, e.g. Dockerfile,.env.*.
    #[arg(long, value_delimiter = ',')]
    names: Vec<String>,
    /// The interpreters of the hashbang lines of its files, comma-separated, e.g. sh,bash.
    #[arg(long, value_delimiter = ',', default_value = "")]
    hashbangs: Vec<String>,
    /// What the first bytes of its files hold, as `"contents"` in its plugin.json (a JSON list).
    #[arg(long, default_value = "")]
    contents: String,
    /// The language servers it declares, as `"languageServers"` in its plugin.json (a JSON list).
    #[arg(long, default_value = "[]")]
    language_servers: String,
    /// The file templates and variables it declares, as `"fileTemplates"` in its plugin.json (a JSON object).
    #[arg(long, default_value = "")]
    file_templates: String,
    /// The live templates, postfix templates, doc comment stubs and statement rules it declares, as `"liveTemplates"` in its plugin.json (a JSON object).
    #[arg(long, default_value = "")]
    live_templates: String,
    /// What it adds to other plug-ins' language servers, as `"serverPlugins"` in its plugin.json (a JSON list).
    #[arg(long, default_value = "")]
    server_plugins: String,
    /// The ids of the plug-ins it needs, comma-separated, e.g. vue.
    #[arg(long, value_delimiter = ',', default_value = "")]
    requires: Vec<String>,
    /// What it gives the projects it supports, as `"project"` in its plugin.json (a JSON object).
    #[arg(long, default_value = "")]
    project: String,
    /// The database drivers it declares, as `"databaseDrivers"` in its plugin.json (a JSON list).
    #[arg(long, default_value = "")]
    database_drivers: String,
    /// The project generators it declares, as `"projectGenerators"` in its plugin.json (a JSON list).
    #[arg(long, default_value = "")]
    project_generators: String,
    /// The tests it declares, as `"tests"` in its plugin.json (a JSON list).
    #[arg(long, default_value = "")]
    tests: String,
    /// The code style pages it declares, as `"codeStyles"` in its plugin.json (a JSON list).
    #[arg(long, default_value = "")]
    code_styles: String,
    /// The terminal agent resume adapters it declares, as `"terminalAgents"` in its plugin.json (a JSON list).
    #[arg(long, default_value = "")]
    terminal_agents: String,
    /// The schemas it ships, as `"schemas"` in its plugin.json (a JSON list).
    #[arg(long, default_value = "")]
    schemas: String,
    /// The programs its run targets may start, as `"runs"` in its plugin.json (comma-separated).
    #[arg(long, value_delimiter = ',', default_value = "")]
    runs: Vec<String>,
    /// The commands only a person starts, as `"personOnly"` in its plugin.json (a JSON list).
    #[arg(long, default_value = "")]
    person_only: String,
    /// The file patterns its run targets also serve, as `"targetFiles"` in its plugin.json (comma-separated).
    #[arg(long, value_delimiter = ',', default_value = "")]
    target_files: Vec<String>,
    /// The project index it declares, as `"index"` in its plugin.json (a JSON object).
    #[arg(long, default_value = "")]
    index: String,
    /// The tool windows it declares, as `"toolWindows"` in its plugin.json (a JSON list).
    #[arg(long, default_value = "")]
    tool_windows: String,
    /// The version, a semantic version such as 0.1.0.
    #[arg(long)]
    version: String,
    /// The Markdown file with the version's release notes, carried by the catalog.
    #[arg(long)]
    notes: Option<PathBuf>,
    /// Stage it under this name instead of publishing it; `release-stage` makes it live (ADR-0171).
    #[arg(long)]
    stage: Option<String>,
    /// The WebAssembly module.
    file: PathBuf,
}

#[derive(Subcommand)]
enum Command {
    /// Create a signing key pair. Writes the secret key to a new file and prints the public key.
    Keygen {
        /// Where to write the secret key. The file must not exist.
        #[arg(long)]
        out: PathBuf,
    },
    /// Serve releases from a data directory. Uploads need the token in ZEN_RELEASES_TOKEN.
    Serve {
        /// The data directory (created if missing).
        #[arg(long, default_value = "releases")]
        data: PathBuf,
        /// The address to listen on. Loopback by default; put a TLS proxy in front for production.
        #[arg(long, default_value = "127.0.0.1:8787")]
        listen: std::net::SocketAddr,
        /// The trusted public key, hex, as printed by `keygen`.
        #[arg(long, env = "ZEN_RELEASES_PUBLIC_KEY")]
        public_key: String,
    },
    /// Withdraw a product, or one version of it, from the signed manifest. Installed copies stay.
    Withdraw {
        /// The server's base URL.
        #[arg(long, env = "ZEN_RELEASES_SERVER", default_value = "http://127.0.0.1:8787")]
        server: String,
        /// The publisher's secret key file; without it, the key is read from ZEN_PUBLISHER_KEY (its
        /// hex, as a release pipeline's protected variable gives it, ADR-0171).
        #[arg(long)]
        key: Option<PathBuf>,
        /// The product id, e.g. rustronin.
        #[arg(long)]
        product: String,
        /// Only this version; every version when absent.
        #[arg(long)]
        version: Option<String>,
    },
    /// Upload a plug-in module and publish it in the signed plug-in catalog (ADR-0031).
    PublishPlugin(Box<PublishPlugin>),
    /// Replace the release notes of a published version in the signed manifest (artifacts and date
    /// stay), from a Markdown file.
    SetNotes {
        /// The server's base URL.
        #[arg(long, env = "ZEN_RELEASES_SERVER", default_value = "http://127.0.0.1:8787")]
        server: String,
        /// The publisher's secret key file; without it, the key is read from ZEN_PUBLISHER_KEY (its
        /// hex, as a release pipeline's protected variable gives it, ADR-0171).
        #[arg(long)]
        key: Option<PathBuf>,
        /// The product id, e.g. toolbox.
        #[arg(long)]
        product: String,
        /// The published version.
        #[arg(long)]
        version: String,
        /// The Markdown file with the notes.
        notes: PathBuf,
    },
    /// Publish the changelogs for the website (the file `cargo xtask changelog --json` writes).
    PublishChangelog {
        /// The server's base URL.
        #[arg(long, env = "ZEN_RELEASES_SERVER", default_value = "http://127.0.0.1:8787")]
        server: String,
        /// Stage it under this name instead of publishing it; `release-stage` makes it live (ADR-0171).
        #[arg(long)]
        stage: Option<String>,
        /// The JSON file.
        file: PathBuf,
    },
    /// Check the signing key and the upload token against the server without publishing anything:
    /// prints only whether the server accepts them (ADR-0171).
    CheckCredentials {
        /// The server's base URL.
        #[arg(long, env = "ZEN_RELEASES_SERVER", default_value = "http://127.0.0.1:8787")]
        server: String,
        /// The publisher's secret key file; without it, ZEN_PUBLISHER_KEY.
        #[arg(long)]
        key: Option<PathBuf>,
    },
    /// Upload artifacts, several at a time, each only when the server does not have it with the same
    /// size and SHA-256. Nothing is listed: a manifest or catalog lists them later.
    Upload {
        /// The server's base URL.
        #[arg(long, env = "ZEN_RELEASES_SERVER", default_value = "http://127.0.0.1:8787")]
        server: String,
        /// How many uploads run at once.
        #[arg(long, default_value_t = 3)]
        jobs: usize,
        /// Each file as `<product>/<version>/<name>=<path>`.
        #[arg(required = true)]
        files: Vec<String>,
    },
    /// Upload a website (every file under a folder) into a stage (ADR-0171).
    StageSite {
        /// The server's base URL.
        #[arg(long, env = "ZEN_RELEASES_SERVER", default_value = "http://127.0.0.1:8787")]
        server: String,
        /// The stage's name.
        #[arg(long)]
        stage: String,
        /// The folder of the website (index.html at its top).
        dir: PathBuf,
    },
    /// Show what the server holds under a stage, as JSON.
    StageStatus {
        /// The server's base URL.
        #[arg(long, env = "ZEN_RELEASES_SERVER", default_value = "http://127.0.0.1:8787")]
        server: String,
        /// The stage's name.
        #[arg(long)]
        stage: String,
    },
    /// Make a stage live at once: its manifest, plug-in catalog, changelogs and website (ADR-0171).
    ReleaseStage {
        /// The server's base URL.
        #[arg(long, env = "ZEN_RELEASES_SERVER", default_value = "http://127.0.0.1:8787")]
        server: String,
        /// The stage's name.
        #[arg(long)]
        stage: String,
    },
    /// Upload an artifact and publish it in the signed manifest.
    Publish {
        /// The server's base URL.
        #[arg(long, env = "ZEN_RELEASES_SERVER", default_value = "http://127.0.0.1:8787")]
        server: String,
        /// The publisher's secret key file; without it, the key is read from ZEN_PUBLISHER_KEY (its
        /// hex, as a release pipeline's protected variable gives it, ADR-0171).
        #[arg(long)]
        key: Option<PathBuf>,
        /// The product id, e.g. rustronin.
        #[arg(long)]
        product: String,
        /// The product's display name, e.g. RustRonin.
        #[arg(long)]
        name: String,
        /// The version, a semantic version such as 0.1.0.
        #[arg(long)]
        version: String,
        /// The channel: stable or nightly.
        #[arg(long, default_value = "stable")]
        channel: String,
        /// The platform; defaults to this machine's.
        #[arg(long)]
        platform: Option<String>,
        /// Release notes in Markdown.
        #[arg(long, default_value = "")]
        notes: String,
        /// Stage it under this name instead of publishing it; `release-stage` makes it live (ADR-0171).
        #[arg(long)]
        stage: Option<String>,
        /// The artifact file.
        file: PathBuf,
    },
}

fn secret(key: Option<&std::path::Path>) -> Result<String, String> {
    match key {
        Some(path) => zen_releases::keys::read_secret(path),
        None => {
            let secret = std::env::var("ZEN_PUBLISHER_KEY")
                .map_err(|_| "give --key <file>, or the key's hex in ZEN_PUBLISHER_KEY".to_owned())?;
            zen_releases::keys::secret_from_text(&secret, "ZEN_PUBLISHER_KEY")
        }
    }
}

fn token() -> Option<String> {
    std::env::var("ZEN_RELEASES_TOKEN").ok().filter(|token| !token.is_empty())
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Keygen { out } => {
            let public = zen_releases::keys::generate(&out)?;
            println!("Secret key written to {} (keep it private).", out.display());
            println!("Public key: {public}");
            Ok(())
        }
        Command::Serve { data, listen, public_key } => {
            let token = token();
            if token.is_none() {
                eprintln!("warning: ZEN_RELEASES_TOKEN is not set; uploads are disabled");
            }
            let config = zen_releases::server::Config { data, public_key, token };
            let runtime = tokio::runtime::Runtime::new().map_err(|error| error.to_string())?;
            println!("Serving releases on http://{listen}/releases/manifest.json");
            runtime.block_on(zen_releases::server::serve(config, listen))
        }
        Command::SetNotes { server, key, product, version, notes } => {
            let token = token().ok_or("set ZEN_RELEASES_TOKEN to the server's upload token")?;
            let secret = secret(key.as_deref())?;
            let notes = std::fs::read_to_string(&notes).map_err(|error| format!("{}: {error}", notes.display()))?;
            if zen_releases::publish::set_notes(&server, &token, &secret, &product, &version, &notes)? {
                println!("{product} {version}: release notes replaced.");
            } else {
                println!("{product} {version}: unchanged (same notes, or not published).");
            }
            Ok(())
        }
        Command::Withdraw { server, key, product, version } => {
            let token = token().ok_or("set ZEN_RELEASES_TOKEN to the server's upload token")?;
            let secret = secret(key.as_deref())?;
            let withdrawn = zen_releases::publish::withdraw(&server, &token, &secret, &product, version.as_deref())?;
            println!("Withdrew {withdrawn} release(s) of {product}.");
            Ok(())
        }
        Command::PublishPlugin(plugin) => {
            let PublishPlugin {
                server,
                key,
                id,
                name,
                description,
                extensions,
                names,
                hashbangs,
                contents,
                language_servers,
                file_templates,
                live_templates,
                server_plugins,
                requires,
                project,
                database_drivers,
                project_generators,
                tests,
                code_styles,
                terminal_agents,
                schemas,
                runs,
                person_only,
                target_files,
                tool_windows,
                index,
                version,
                notes,
                stage,
                file,
            } = *plugin;
            let token = token().ok_or("set ZEN_RELEASES_TOKEN to the server's upload token")?;
            let secret = secret(key.as_deref())?;
            let notes = match notes {
                Some(path) => std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?,
                None => String::new(),
            };
            let keep = |list: Vec<String>| list.into_iter().filter(|item| !item.trim().is_empty()).collect::<Vec<_>>();
            let (extensions, names, hashbangs, requires, runs) =
                (keep(extensions), keep(names), keep(hashbangs), keep(requires), keep(runs));
            let language_servers = zen_plugins::ServerDeclaration::parse_list(&language_servers)?;
            let file_templates =
                zen_plugins::FileTemplateDeclarations::parse(&file_templates).map_err(|error| error.to_string())?;
            let live_templates =
                zen_plugins::LiveTemplateDeclarations::parse(&live_templates).map_err(|error| error.to_string())?;
            let project = zen_plugins::ProjectSupport::parse(&project)?;
            let contents = zen_plugins::ContentDeclaration::parse_list(&contents).map_err(|error| error.to_string())?;
            let server_plugins = zen_plugins::ServerPlugin::parse_list(&server_plugins)?;
            let database_drivers: Vec<serde_json::Value> = if database_drivers.trim().is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(&database_drivers).map_err(|error| format!("--database-drivers: {error}"))?
            };
            let project_generators = zen_plugins::GeneratorDeclaration::parse_list(&project_generators)?;
            let tests: Vec<serde_json::Value> = if tests.trim().is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(&tests).map_err(|error| format!("--tests: {error}"))?
            };
            let code_styles = zen_plugins::CodeStyleDeclaration::parse_list(&code_styles)?;
            let terminal_agents: Vec<serde_json::Value> = if terminal_agents.trim().is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(&terminal_agents).map_err(|error| format!("--terminal-agents: {error}"))?
            };
            let index: Option<zen_plugins::IndexDeclaration> = if index.trim().is_empty() {
                None
            } else {
                Some(serde_json::from_str(&index).map_err(|error| format!("--index: {error}"))?)
            };
            let tool_windows: Vec<zen_plugins::ToolWindowDeclaration> = if tool_windows.trim().is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(&tool_windows).map_err(|error| format!("--tool-windows: {error}"))?
            };
            let schemas: Vec<zen_plugins::SchemaDeclaration> = if schemas.trim().is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(&schemas).map_err(|error| format!("--schemas: {error}"))?
            };
            let person_only: Vec<zen_plugins::PersonOnly> = if person_only.trim().is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(&person_only).map_err(|error| format!("--person-only: {error}"))?
            };
            let plugin = zen_plugins::CatalogPlugin {
                id,
                name: name.clone(),
                description,
                extensions,
                names,
                hashbangs,
                contents,
                language_servers,
                file_templates,
                live_templates,
                server_plugins,
                requires,
                project,
                database_drivers,
                project_generators,
                tests,
                terminal_agents,
                schemas,
                runs,
                person_only,
                target_files: target_files.into_iter().filter(|pattern| !pattern.is_empty()).collect(),
                tool_windows,
                index,
                code_styles,
                versions: Vec::new(),
            };
            let publication = zen_releases::publish::PluginPublication {
                server: &server,
                token: &token,
                secret_key: &secret,
                plugin,
                version: &version,
                file: &file,
                notes: &notes,
                stage: stage.as_deref(),
            };
            zen_releases::publish::publish_plugin(&publication)?;
            println!("Published the {name} plug-in {version}.");
            Ok(())
        }
        Command::PublishChangelog { server, stage, file } => {
            let token = token().ok_or("set ZEN_RELEASES_TOKEN to the server's upload token")?;
            let json = std::fs::read(&file).map_err(|error| format!("cannot read {}: {error}", file.display()))?;
            zen_releases::publish::stage_changelog(&server, &token, stage.as_deref(), &json)?;
            match stage {
                Some(stage) => println!("The changelogs are staged under {stage}."),
                None => println!("The changelogs are published at {server}/releases/changelog.json."),
            }
            Ok(())
        }
        Command::CheckCredentials { server, key } => {
            let token = token().ok_or("set ZEN_RELEASES_TOKEN to the server's upload token")?;
            let secret = secret(key.as_deref())?;
            zen_releases::publish::check_credentials(&server, &token, &secret)?;
            println!("ok: {server} accepts the token and trusts the signing key; nothing was published.");
            Ok(())
        }
        Command::Upload { server, jobs, files } => {
            let token = token().ok_or("set ZEN_RELEASES_TOKEN to the server's upload token")?;
            let uploads = files
                .iter()
                .map(|entry| {
                    let (to, path) =
                        entry.split_once('=').ok_or(format!("`{entry}`: give <product>/<version>/<name>=<path>"))?;
                    let parts: Vec<&str> = to.split('/').collect();
                    let [product, version, name] = parts.as_slice() else {
                        return Err(format!("`{entry}`: give <product>/<version>/<name>=<path>"));
                    };
                    Ok(zen_releases::publish::Upload {
                        product: (*product).to_owned(),
                        version: (*version).to_owned(),
                        name: (*name).to_owned(),
                        path: PathBuf::from(path),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            let started = std::time::Instant::now();
            let (sent, kept) = zen_releases::publish::upload_all(&server, &token, &uploads, jobs)?;
            println!(
                "Uploaded {sent} files, {kept} already on the server, in {:.0} s ({} at a time, files over 20 MB one at a time).",
                started.elapsed().as_secs_f64(),
                zen_releases::publish::parallel(jobs)
            );
            Ok(())
        }
        Command::StageSite { server, stage, dir } => {
            let token = token().ok_or("set ZEN_RELEASES_TOKEN to the server's upload token")?;
            let count = zen_releases::publish::stage_site(&server, &token, &stage, &dir)?;
            println!("Staged {count} website files; see {server}/releases/staged-site/{stage}/index.html");
            Ok(())
        }
        Command::StageStatus { server, stage } => {
            let token = token().ok_or("set ZEN_RELEASES_TOKEN to the server's upload token")?;
            let summary = zen_releases::publish::stage_summary(&server, &token, &stage)?;
            println!("{}", serde_json::to_string_pretty(&summary).map_err(|error| error.to_string())?);
            Ok(())
        }
        Command::ReleaseStage { server, stage } => {
            let token = token().ok_or("set ZEN_RELEASES_TOKEN to the server's upload token")?;
            let summary = zen_releases::publish::release_stage(&server, &token, &stage)?;
            println!("{}", serde_json::to_string_pretty(&summary).map_err(|error| error.to_string())?);
            Ok(())
        }
        Command::Publish { server, key, product, name, version, channel, platform, notes, stage, file } => {
            let token = token().ok_or("set ZEN_RELEASES_TOKEN to the server's upload token")?;
            let secret = secret(key.as_deref())?;
            let platform = platform.unwrap_or_else(zen_update::current_platform);
            let publication = zen_releases::publish::Publication {
                server: &server,
                token: &token,
                secret_key: &secret,
                product: &product,
                name: &name,
                version: &version,
                channel: &channel,
                platform: &platform,
                notes: &notes,
                file: &file,
                stage: stage.as_deref(),
            };
            zen_releases::publish::publish(&publication)?;
            match stage {
                Some(stage) => println!("Staged {name} {version} ({channel}, {platform}) under {stage}."),
                None => println!("Published {name} {version} ({channel}, {platform})."),
            }
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    zen_update::install_tls();
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("zen-releases: {error}");
            ExitCode::FAILURE
        }
    }
}
