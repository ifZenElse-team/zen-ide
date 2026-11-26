use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

pub use fluent_bundle::FluentValue as Value;

pub const SOURCE: &str = "en";

pub const LANGUAGES: &[(&str, &str)] = &[("en", "English"), ("ru", "Русский")];

pub const SYSTEM: &str = "system";

struct State {
    sources: Vec<(&'static str, &'static str)>,
    chosen: Option<String>,
    bundles: Bundles,
    others: HashMap<String, Bundles>,
}

type Bundles = Arc<Vec<FluentBundle<FluentResource>>>;

static STATE: LazyLock<RwLock<State>> = LazyLock::new(|| {
    RwLock::new(State { sources: Vec::new(), chosen: None, bundles: Arc::new(Vec::new()), others: HashMap::new() })
});

thread_local! {
    static THREAD_LANGUAGE: RefCell<Option<String>> = const { RefCell::new(None) };
}

#[cfg(feature = "thread-languages")]
thread_local! {
    static THREAD_CHOSEN: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn thread_effective() -> Option<String> {
    #[cfg(feature = "thread-languages")]
    {
        Some(match THREAD_CHOSEN.with(|chosen| chosen.borrow().clone()) {
            None => SOURCE.to_owned(),
            Some(chosen) if chosen == SYSTEM => system_language(),
            Some(chosen) => chosen,
        })
    }
    #[cfg(not(feature = "thread-languages"))]
    {
        None
    }
}

fn thread_language() -> Option<String> {
    THREAD_LANGUAGE.with(|language| language.borrow().clone()).or_else(thread_effective)
}

fn read() -> std::sync::RwLockReadGuard<'static, State> {
    STATE.read().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn write() -> std::sync::RwLockWriteGuard<'static, State> {
    STATE.write().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn register(sources: &[(&'static str, &'static str)]) {
    let mut state = write();
    let before = state.sources.len();
    for source in sources {
        if !state.sources.iter().any(|existing| existing.0 == source.0 && std::ptr::eq(existing.1, source.1)) {
            state.sources.push(*source);
        }
    }
    if state.sources.len() != before {
        rebuild(&mut state);
    }
}

pub fn set_language(code: Option<&str>) {
    let code = code.filter(|code| supported(code) || *code == SYSTEM).map(str::to_owned);
    #[cfg(feature = "thread-languages")]
    THREAD_CHOSEN.with(|chosen| *chosen.borrow_mut() = code);
    #[cfg(not(feature = "thread-languages"))]
    {
        let mut state = write();
        state.chosen = code;
        rebuild(&mut state);
    }
}

pub fn chosen_language() -> Option<String> {
    #[cfg(feature = "thread-languages")]
    let chosen = THREAD_CHOSEN.with(|chosen| chosen.borrow().clone());
    #[cfg(not(feature = "thread-languages"))]
    let chosen = read().chosen.clone();
    chosen
}

pub fn language() -> String {
    thread_language().unwrap_or_else(|| effective(&read()))
}

pub fn interface_language() -> String {
    thread_effective().unwrap_or_else(|| effective(&read()))
}

pub fn with_language<R>(code: &str, work: impl FnOnce() -> R) -> R {
    struct Restore(Option<String>);
    impl Drop for Restore {
        fn drop(&mut self) {
            THREAD_LANGUAGE.with(|language| *language.borrow_mut() = self.0.take());
        }
    }
    let code = if supported(code) { code } else { SOURCE };
    let _restore = Restore(THREAD_LANGUAGE.with(|language| language.replace(Some(code.to_owned()))));
    work()
}

fn effective(state: &State) -> String {
    match state.chosen.as_deref() {
        None => SOURCE.to_owned(),
        Some(SYSTEM) => system_language(),
        Some(code) => code.to_owned(),
    }
}

pub fn system_language() -> String {
    sys_locale::get_locales()
        .filter_map(|locale| locale.split(['-', '_']).next().map(str::to_lowercase))
        .find(|code| supported(code))
        .unwrap_or_else(|| SOURCE.to_owned())
}

fn supported(code: &str) -> bool {
    LANGUAGES.iter().any(|(known, _)| *known == code)
}

fn rebuild(state: &mut State) {
    state.others.clear();
    state.bundles = build(state, &effective(state));
}

fn build(state: &State, language: &str) -> Bundles {
    let mut chain = vec![language.to_owned()];
    if language != SOURCE {
        chain.push(SOURCE.to_owned());
    }
    let bundles = chain
        .iter()
        .map(|code| {
            let id: LanguageIdentifier = code.parse().unwrap_or_default();
            let mut bundle = FluentBundle::new_concurrent(vec![id]);
            bundle.set_use_isolating(false);
            for (_, text) in state.sources.iter().filter(|(source_language, _)| source_language == code) {
                let resource = FluentResource::try_new((*text).to_owned()).unwrap_or_else(|(partial, _)| partial);
                let _ = bundle.add_resource(resource);
            }
            bundle
        })
        .collect();
    Arc::new(bundles)
}

fn current_bundles() -> Bundles {
    let Some(language) = thread_language() else {
        return Arc::clone(&read().bundles);
    };
    {
        let state = read();
        if effective(&state) == language {
            return Arc::clone(&state.bundles);
        }
        if let Some(bundles) = state.others.get(&language) {
            return Arc::clone(bundles);
        }
    }
    let mut state = write();
    let bundles = build(&state, &language);
    state.others.insert(language, Arc::clone(&bundles));
    bundles
}

pub fn text(id: &str, args: &[(&str, FluentValue<'_>)]) -> String {
    let bundles = current_bundles();
    let mut fluent_args = FluentArgs::new();
    for (name, value) in args {
        fluent_args.set(*name, value.clone());
    }
    for bundle in bundles.iter() {
        if let Some(pattern) = bundle.get_message(id).and_then(|message| message.value()) {
            let mut errors = Vec::new();
            let formatted: Cow<'_, str> = bundle.format_pattern(pattern, Some(&fluent_args), &mut errors);
            return formatted.into_owned();
        }
    }
    id.to_owned()
}

pub fn source_text(id: &str, args: &[(&str, FluentValue<'_>)]) -> String {
    let bundles = Arc::clone(&read().bundles);
    let Some(bundle) = bundles.last() else { return id.to_owned() };
    let Some(pattern) = bundle.get_message(id).and_then(|message| message.value()) else { return id.to_owned() };
    let mut fluent_args = FluentArgs::new();
    for (name, value) in args {
        fluent_args.set(*name, value.clone());
    }
    let mut errors = Vec::new();
    bundle.format_pattern(pattern, Some(&fluent_args), &mut errors).into_owned()
}

#[derive(Clone, Debug, PartialEq)]
enum Arg {
    Text(String),
    Number(f64),
    Message(Box<Message>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    id: Option<String>,
    args: Vec<(String, Arg)>,
    plain: String,
}

impl Message {
    pub fn new(id: &str) -> Self {
        Self { id: Some(id.to_owned()), args: Vec::new(), plain: String::new() }
    }

    pub fn with(mut self, name: &str, value: impl Into<String>) -> Self {
        self.args.push((name.to_owned(), Arg::Text(value.into())));
        self
    }

    pub fn with_number(mut self, name: &str, value: f64) -> Self {
        self.args.push((name.to_owned(), Arg::Number(value)));
        self
    }

    pub fn with_message(mut self, name: &str, value: Message) -> Self {
        self.args.push((name.to_owned(), Arg::Message(Box::new(value))));
        self
    }

    pub fn plain(text: impl Into<String>) -> Self {
        Self { id: None, args: Vec::new(), plain: text.into() }
    }

    pub fn text(&self) -> String {
        let Some(id) = &self.id else { return self.plain.clone() };
        let args: Vec<(&str, FluentValue<'_>)> = self
            .args
            .iter()
            .map(|(name, value)| {
                let value = match value {
                    Arg::Text(text) => FluentValue::from(text.as_str()),
                    Arg::Number(number) => FluentValue::from(*number),
                    Arg::Message(message) => FluentValue::from(message.text()),
                };
                (name.as_str(), value)
            })
            .collect();
        text(id, &args)
    }
}

#[macro_export]
macro_rules! t {
    ($id:expr) => {
        $crate::text($id, &[])
    };
    ($id:expr, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::text($id, &[$((stringify!($name), $crate::Value::from($value))),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    const EN: &str = "updates = { $count ->\n    [one] One update\n   *[other] { $count } updates\n}\nonly-english = Only in English\n";
    const RU: &str = "updates = { $count ->\n    [one] { $count } обновление\n    [few] { $count } обновления\n   *[many] { $count } обновлений\n}\n";

    #[test]
    fn plurals_and_fallback() {
        register(&[("en", EN), ("ru", RU)]);
        set_language(Some("ru"));
        assert_eq!(t!("updates", count = 1), "1 обновление");
        assert_eq!(t!("updates", count = 3), "3 обновления");
        assert_eq!(t!("updates", count = 5), "5 обновлений");
        assert_eq!(t!("only-english"), "Only in English", "missing translations fall back to English");
        assert_eq!(t!("no-such-message"), "no-such-message");
        let kept = Message::new("updates").with_number("count", 3.0);
        assert_eq!(kept.text(), "3 обновления", "a kept message is worded when it is read");
        assert_eq!(with_language("en", || kept.text()), "3 updates");
        assert_eq!(Message::plain("as it came").text(), "as it came");
        assert_eq!(source_text("only-english", &[]), "Only in English");
        set_language(Some("en"));
        assert_eq!(t!("updates", count = 1), "One update");
        set_language(Some("xx"));
        assert_eq!(chosen_language(), None, "unknown languages are English");

        set_language(None);
        assert_eq!((chosen_language(), interface_language()), (None, "en".to_owned()), "no choice: English");
        set_language(Some(SYSTEM));
        assert_eq!(chosen_language().as_deref(), Some(SYSTEM));
        assert_eq!(interface_language(), system_language(), "the system's language only when chosen");
        set_language(Some("ru"));
        assert_eq!(interface_language(), "ru");
        set_language(Some("en"));
    }

    #[test]
    fn a_thread_can_speak_another_language_for_a_while() {
        register(&[("en", EN), ("ru", RU)]);
        let answer = with_language("ru", || {
            let inner = with_language("en", || (t!("updates", count = 1), language()));
            (t!("updates", count = 5), language(), inner)
        });
        assert_eq!(answer, ("5 обновлений".to_owned(), "ru".to_owned(), ("One update".to_owned(), "en".to_owned())));
        assert_eq!(with_language("xx", language), SOURCE, "an unknown language is English");
        assert_eq!(
            std::thread::spawn(|| with_language("ru", || t!("only-english"))).join().unwrap(),
            "Only in English",
            "missing translations fall back to English"
        );
        let other_thread = with_language("ru", || std::thread::spawn(|| THREAD_LANGUAGE.with(|l| l.borrow().clone())));
        assert_eq!(other_thread.join().unwrap(), None, "other threads keep their language");
    }

    #[cfg(feature = "thread-languages")]
    #[test]
    fn with_thread_languages_each_thread_chooses_its_own_and_starts_in_english() {
        register(&[("en", EN), ("ru", RU)]);
        assert_eq!((language(), chosen_language()), (SOURCE.to_owned(), None), "English, whatever the system's");
        let (sender, receiver) = std::sync::mpsc::channel();
        let (resume, wait) = std::sync::mpsc::channel::<()>();
        let russian = std::thread::spawn(move || {
            set_language(Some("ru"));
            sender.send(t!("updates", count = 5)).unwrap();
            wait.recv().unwrap();
            t!("updates", count = 5)
        });
        assert_eq!(receiver.recv().unwrap(), "5 обновлений");
        assert_eq!(t!("updates", count = 5), "5 updates", "another thread's choice is not this one's");
        set_language(Some("en"));
        resume.send(()).unwrap();
        assert_eq!(russian.join().unwrap(), "5 обновлений", "nor is this thread's that one's");
    }
}
