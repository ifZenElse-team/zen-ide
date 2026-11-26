use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError, RwLock};

use log::{Level, LevelFilter, Log, Metadata, Record};

pub const FILE_NAME: &str = "zen.log";
pub const CATEGORIES_FILE: &str = "log-categories.txt";
pub const ROTATED: usize = 12;
const LIMIT: u64 = 5 * 1024 * 1024;

static INSTALLED: OnceLock<FileLog> = OnceLock::new();

pub fn folder(app_id: &str) -> Option<PathBuf> {
    let env = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    if let Some(data) = env("ZEN_DATA_DIR") {
        return Some(data.join(app_id).join("logs"));
    }
    if cfg!(target_os = "macos") {
        env("HOME").map(|home| home.join("Library/Logs").join(app_id))
    } else if cfg!(windows) {
        env("LOCALAPPDATA").map(|base| base.join(app_id).join("logs"))
    } else {
        let state = env("XDG_STATE_HOME").or_else(|| env("HOME").map(|home| home.join(".local/state")));
        state.map(|base| base.join(app_id).join("logs"))
    }
}

pub fn installed_folder() -> Option<&'static Path> {
    INSTALLED.get().map(|log| log.folder.as_path())
}

pub fn rotated_name(number: usize) -> String {
    format!("zen.{number}.log")
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Category {
    prefix: String,
    level: LevelFilter,
}

fn parse_categories(lines: &[String]) -> Vec<Category> {
    lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(|line| match line.strip_suffix(":trace") {
            Some(prefix) => Category { prefix: prefix.to_owned(), level: LevelFilter::Trace },
            None => Category { prefix: line.to_owned(), level: LevelFilter::Debug },
        })
        .collect()
}

fn level_of(categories: &[Category], target: &str) -> LevelFilter {
    let matching = categories.iter().filter(|category| {
        target.strip_prefix(category.prefix.as_str()).is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
    });
    match matching.max_by_key(|category| category.prefix.len()) {
        Some(category) => category.level,
        None if target.starts_with("zen") => LevelFilter::Info,
        None => LevelFilter::Warn,
    }
}

pub(crate) fn private(options: &mut std::fs::OpenOptions) -> &mut std::fs::OpenOptions {
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(options, 0o600);
    options
}

struct Writer {
    file: Option<File>,
    written: u64,
}

pub struct FileLog {
    folder: PathBuf,
    writer: Mutex<Writer>,
    categories: RwLock<Vec<Category>>,
}

impl FileLog {
    fn open(folder: PathBuf, categories: &[String]) -> Self {
        let log = Self {
            writer: Mutex::new(Writer { file: None, written: 0 }),
            categories: RwLock::new(parse_categories(categories)),
            folder,
        };
        let _ = std::fs::create_dir_all(&log.folder);
        log.reopen(&mut log.writer.lock().unwrap_or_else(PoisonError::into_inner));
        log
    }

    fn reopen(&self, writer: &mut Writer) {
        let path = self.folder.join(FILE_NAME);
        writer.written = std::fs::metadata(&path).map_or(0, |metadata| metadata.len());
        writer.file = private(File::options().create(true).append(true)).open(path).ok();
    }

    fn rotate(&self, writer: &mut Writer) {
        writer.file = None;
        let _ = std::fs::remove_file(self.folder.join(rotated_name(ROTATED)));
        for number in (1..ROTATED).rev() {
            let _ = std::fs::rename(self.folder.join(rotated_name(number)), self.folder.join(rotated_name(number + 1)));
        }
        let _ = std::fs::rename(self.folder.join(FILE_NAME), self.folder.join(rotated_name(1)));
        self.reopen(writer);
    }

    fn write_line(&self, line: &str) {
        let mut writer = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
        if writer.written + line.len() as u64 > LIMIT && writer.written > 0 {
            self.rotate(&mut writer);
        }
        if let Some(file) = &mut writer.file
            && file.write_all(line.as_bytes()).is_ok()
        {
            writer.written += line.len() as u64;
        }
    }

    fn set_categories(&self, lines: &[String]) {
        let categories = parse_categories(lines);
        log::set_max_level(max_level(&categories));
        *self.categories.write().unwrap_or_else(PoisonError::into_inner) = categories;
    }

    fn write_panic(&self, info: &std::panic::PanicHookInfo<'_>) {
        let thread = std::thread::current();
        let backtrace = std::backtrace::Backtrace::force_capture();
        self.write_line(&line(
            Level::Error,
            "panic",
            &format!("thread '{}': {info}\n{backtrace}", thread.name().unwrap_or("unnamed")),
        ));
    }
}

fn max_level(categories: &[Category]) -> LevelFilter {
    categories.iter().map(|category| category.level).max().unwrap_or(LevelFilter::Info).max(LevelFilter::Info)
}

pub fn without_credentials(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("://") {
        let (head, tail) = rest.split_at(at + 3);
        out.push_str(head);
        let end =
            tail.find(|c: char| c.is_whitespace() || matches!(c, '/' | '"' | '\'' | '?' | '#')).unwrap_or(tail.len());
        match tail[..end].rfind('@') {
            Some(sign) => {
                out.push_str("***");
                out.push_str(&tail[sign..end]);
            }
            None => out.push_str(&tail[..end]),
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

fn line(level: Level, target: &str, message: &str) -> String {
    let now = jiff::Timestamp::now().strftime("%Y-%m-%dT%H:%M:%S%.3fZ");
    let message = without_credentials(message);
    format!("{now} {level:<5} {target} {message}\n")
}

impl Log for FileLog {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        let categories = self.categories.read().unwrap_or_else(PoisonError::into_inner);
        metadata.level() <= level_of(&categories, metadata.target())
    }

    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata()) {
            self.write_line(&line(record.level(), record.target(), &record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

pub fn install(app_id: &str, product: &str, version: &str, data: Option<&Path>) {
    let Some(folder) = folder(app_id) else { return };
    let lines = data.map(read_categories).unwrap_or_default();
    let log = INSTALLED.get_or_init(|| FileLog::open(folder, &lines));
    if log::set_logger(log).is_err() {
        return;
    }
    log::set_max_level(max_level(&parse_categories(&lines)));
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(log) = INSTALLED.get() {
            log.write_panic(info);
        }
        previous(info);
    }));
    log::info!("{product} {version} started on {} {}", std::env::consts::OS, std::env::consts::ARCH);
}

pub fn read_categories(data: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(data.join(CATEGORIES_FILE)).unwrap_or_default();
    text.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned).collect()
}

pub fn write_categories(data: &Path, lines: &[String]) -> std::io::Result<()> {
    let mut text = lines.join("\n");
    text.push('\n');
    std::fs::write(data.join(CATEGORIES_FILE), text)?;
    if let Some(log) = INSTALLED.get() {
        log.set_categories(lines);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
