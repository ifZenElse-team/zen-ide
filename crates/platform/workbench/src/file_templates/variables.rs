use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const PREDEFINED: &[(&str, &str)] = &[
    ("NAME", "workbench-file-templates-var-name"),
    ("FILE_NAME", "workbench-file-templates-var-file-name"),
    ("DIR_PATH", "workbench-file-templates-var-dir-path"),
    ("PACKAGE_NAME", "workbench-file-templates-var-package-name"),
    ("PROJECT_NAME", "workbench-file-templates-var-project-name"),
    ("PRODUCT_NAME", "workbench-file-templates-var-product-name"),
    ("USER", "workbench-file-templates-var-user"),
    ("DATE", "workbench-file-templates-var-date"),
    ("TIME", "workbench-file-templates-var-time"),
    ("YEAR", "workbench-file-templates-var-year"),
    ("MONTH", "workbench-file-templates-var-month"),
    ("MONTH_NAME_SHORT", "workbench-file-templates-var-month-name-short"),
    ("MONTH_NAME_FULL", "workbench-file-templates-var-month-name-full"),
    ("DAY", "workbench-file-templates-var-day"),
    ("DAY_NAME_SHORT", "workbench-file-templates-var-day-name-short"),
    ("DAY_NAME_FULL", "workbench-file-templates-var-day-name-full"),
    ("HOUR", "workbench-file-templates-var-hour"),
    ("MINUTE", "workbench-file-templates-var-minute"),
    ("SECOND", "workbench-file-templates-var-second"),
    ("DS", "workbench-file-templates-var-ds"),
];

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

const DAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Moment {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub weekday: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl Moment {
    pub fn at(time: SystemTime, offset: i32) -> Self {
        let seconds = match time.duration_since(UNIX_EPOCH) {
            Ok(since) => i64::try_from(since.as_secs()).unwrap_or(i64::MAX / 2),
            Err(before) => -i64::try_from(before.duration().as_secs()).unwrap_or(i64::MAX / 2),
        } + i64::from(offset);
        let days = seconds.div_euclid(86_400);
        let in_day = seconds.rem_euclid(86_400);
        let (year, month, day) = civil(days);
        Self {
            year,
            month,
            day,
            weekday: u32::try_from((days + 3).rem_euclid(7)).unwrap_or(0),
            hour: u32::try_from(in_day / 3600).unwrap_or(0),
            minute: u32::try_from(in_day % 3600 / 60).unwrap_or(0),
            second: u32::try_from(in_day % 60).unwrap_or(0),
        }
    }

    pub fn now() -> Self {
        Self::at(SystemTime::now(), zen_vcs::local_offset())
    }
}

fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, u32::try_from(month).unwrap_or(1), u32::try_from(day).unwrap_or(1))
}

#[derive(Clone, Debug)]
pub struct Context<'a> {
    pub name: &'a str,
    pub file_name: &'a str,
    pub directory: &'a Path,
    pub project: Option<&'a Path>,
    pub product: &'a str,
    pub now: Moment,
}

pub fn project_name(project: &Path) -> String {
    if let Ok(name) = std::fs::read_to_string(project.join(".idea").join(".name")) {
        let name = name.trim();
        if !name.is_empty() {
            return name.to_owned();
        }
    }
    project.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn user() -> String {
    ["USER", "USERNAME", "LOGNAME"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|v| !v.is_empty()))
        .unwrap_or_default()
}

pub fn predefined(context: &Context<'_>) -> HashMap<String, String> {
    let now = context.now;
    let dir_path = match context.project {
        Some(project) => context.directory.strip_prefix(project).unwrap_or(context.directory),
        None => context.directory,
    };
    let dir_path = dir_path.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
    let month = MONTHS[(now.month.clamp(1, 12) - 1) as usize];
    let day = DAYS[(now.weekday % 7) as usize];
    let values = [
        ("NAME", context.name.to_owned()),
        ("FILE_NAME", context.file_name.to_owned()),
        ("PACKAGE_NAME", package_name(&dir_path)),
        ("DIR_PATH", dir_path),
        ("PROJECT_NAME", context.project.map(project_name).unwrap_or_default()),
        ("PRODUCT_NAME", context.product.to_owned()),
        ("USER", user()),
        ("DATE", format!("{:04}-{:02}-{:02}", now.year, now.month, now.day)),
        ("TIME", format!("{:02}:{:02}", now.hour, now.minute)),
        ("YEAR", format!("{:04}", now.year)),
        ("MONTH", format!("{:02}", now.month)),
        ("MONTH_NAME_SHORT", month[..3].to_owned()),
        ("MONTH_NAME_FULL", month.to_owned()),
        ("DAY", format!("{:02}", now.day)),
        ("DAY_NAME_SHORT", day[..3].to_owned()),
        ("DAY_NAME_FULL", day.to_owned()),
        ("HOUR", format!("{:02}", now.hour)),
        ("MINUTE", format!("{:02}", now.minute)),
        ("SECOND", format!("{:02}", now.second)),
        ("DS", "$".to_owned()),
    ];
    values.into_iter().map(|(name, value)| (name.to_owned(), value)).collect()
}

pub fn package_name(dir_path: &str) -> String {
    let parts: Vec<&str> = dir_path.split('/').filter(|part| !part.is_empty()).collect();
    let Some(src) = parts.iter().position(|part| *part == "src") else { return String::new() };
    let root = parts[src..]
        .iter()
        .rposition(|part| matches!(*part, "java" | "kotlin" | "scala" | "groovy"))
        .map_or(src, |at| src + at);
    let package = &parts[root + 1..];
    if package.iter().all(|part| part.chars().all(|c| c.is_alphanumeric() || c == '_')) {
        package.join(".")
    } else {
        String::new()
    }
}

pub fn is_predefined(name: &str) -> bool {
    PREDEFINED.iter().any(|(known, _)| *known == name)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    #[test]
    fn the_package_name_is_the_folders_below_the_source_root() {
        assert_eq!(package_name("src/main/java/com/acme/shop"), "com.acme.shop");
        assert_eq!(package_name("app/src/test/java/org/demo"), "org.demo");
        assert_eq!(package_name("src/main/java"), "");
        assert_eq!(package_name("src/org/demo"), "org.demo");
        assert_eq!(package_name("docs/guide"), "");
        assert_eq!(package_name("src/main/java/my-folder"), "", "not a package name");
    }

    use super::*;

    #[test]
    fn a_moment_has_the_calendar_and_the_clock_of_its_time_zone() {
        let thursday = UNIX_EPOCH + Duration::from_secs(1_792_639_509);
        let moment = Moment::at(thursday, 0);
        assert_eq!(
            moment,
            Moment { year: 2026, month: 10, day: 22, weekday: 3, hour: 3, minute: 25, second: 9 },
            "2026-10-22T03:25:09Z"
        );
        let east = Moment::at(thursday, 21 * 3600);
        assert_eq!((east.day, east.weekday, east.hour), (23, 4, 0), "a zone ahead may be on the next day");
        let leap = Moment::at(UNIX_EPOCH + Duration::from_secs(951_782_400), 0);
        assert_eq!((leap.year, leap.month, leap.day, leap.weekday), (2000, 2, 29, 1));
        let before = Moment::at(UNIX_EPOCH - Duration::from_secs(1), 0);
        assert_eq!((before.year, before.month, before.day, before.hour, before.second), (1969, 12, 31, 23, 59));
    }

    #[test]
    fn the_predefined_variables_name_the_file_its_folder_and_the_moment() {
        let now = Moment { year: 2026, month: 3, day: 7, weekday: 5, hour: 9, minute: 4, second: 2 };
        let project = Path::new("/work/shop");
        let context = Context {
            name: "order",
            file_name: "order.go",
            directory: &project.join("internal/orders"),
            project: Some(project),
            product: "GoGarden",
            now,
        };
        let values = predefined(&context);
        let get = |name: &str| values.get(name).map(String::as_str).unwrap();
        assert_eq!(get("NAME"), "order");
        assert_eq!(get("FILE_NAME"), "order.go");
        assert_eq!(get("DIR_PATH"), "internal/orders");
        assert_eq!(get("PROJECT_NAME"), "shop");
        assert_eq!(get("PRODUCT_NAME"), "GoGarden");
        assert_eq!(get("DATE"), "2026-03-07");
        assert_eq!(get("TIME"), "09:04");
        assert_eq!((get("YEAR"), get("MONTH"), get("DAY")), ("2026", "03", "07"));
        assert_eq!((get("MONTH_NAME_SHORT"), get("MONTH_NAME_FULL")), ("Mar", "March"));
        assert_eq!((get("DAY_NAME_SHORT"), get("DAY_NAME_FULL")), ("Sat", "Saturday"));
        assert_eq!((get("HOUR"), get("MINUTE"), get("SECOND")), ("09", "04", "02"));
        assert_eq!(get("DS"), "$");
        assert_eq!(get("USER"), user());
        for (name, _) in PREDEFINED {
            assert!(values.contains_key(*name), "{name} has a value");
        }
        let at_root = Context { directory: project, ..context };
        assert_eq!(predefined(&at_root)["DIR_PATH"], "");
    }
}
