use std::path::PathBuf;

const SETTLE_FRAMES: u64 = 12;

const SETTLE_SECONDS: f64 = 3.0;

static REQUESTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

type BeforeQuit = Box<dyn Fn() + Send>;

static BEFORE_QUIT: std::sync::Mutex<Option<BeforeQuit>> = std::sync::Mutex::new(None);

pub fn before_quit(hook: impl Fn() + Send + 'static) {
    *BEFORE_QUIT.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(hook));
}

pub fn profile_begin(ctx: &egui::Context) -> Option<std::time::Instant> {
    let mode = std::env::var("ZEN_UI_PROFILE").ok()?;
    ctx.request_repaint();
    match mode.as_str() {
        "scroll" => {
            let t = ctx.input(|input| input.time);
            let direction = if ((t / 3.0) as u64).is_multiple_of(2) { -1.0 } else { 1.0 };
            ctx.input_mut(|input| input.smooth_scroll_delta.y = 40.0 * direction);
        }
        "type" => {
            let event = if ctx.cumulative_frame_nr().is_multiple_of(2) {
                egui::Event::Text("x".to_owned())
            } else {
                egui::Event::Key {
                    key: egui::Key::Backspace,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }
            };
            ctx.input_mut(|input| input.events.push(event));
        }
        _ => {}
    }
    Some(std::time::Instant::now())
}

static LEXED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn note_lexed(lines: usize) {
    LEXED.fetch_max(lines, std::sync::atomic::Ordering::Relaxed);
}

pub fn profile_end(started: Option<std::time::Instant>) {
    static STATE: std::sync::Mutex<(Option<std::time::Instant>, Vec<f64>)> = std::sync::Mutex::new((None, Vec::new()));
    let Some(started) = started else { return };
    let now = std::time::Instant::now();
    let Ok(mut state) = STATE.lock() else { return };
    state.1.push(now.duration_since(started).as_secs_f64() * 1000.0);
    let since = *state.0.get_or_insert(now);
    if now.duration_since(since).as_secs_f64() < 2.0 {
        return;
    }
    let mut times = std::mem::take(&mut state.1);
    state.0 = Some(now);
    times.sort_by(f64::total_cmp);
    let at = |q: f64| times[((times.len() - 1) as f64 * q).round() as usize];
    let mean = times.iter().sum::<f64>() / times.len() as f64;
    let mode = std::env::var("ZEN_UI_PROFILE").unwrap_or_default();
    #[allow(clippy::print_stderr, reason = "a developer tool, read by cargo xtask bench-editor")]
    {
        eprintln!(
            "profile {mode}: frames={} mean={mean:.2} p50={:.2} p95={:.2} max={:.2} ms lexed={}",
            times.len(),
            at(0.5),
            at(0.95),
            at(1.0),
            LEXED.swap(0, std::sync::atomic::Ordering::Relaxed)
        );
    }
}

pub fn tick(ctx: &egui::Context) {
    let Some(target) = std::env::var_os("ZEN_UI_SCREENSHOT").map(PathBuf::from) else { return };
    let delay =
        std::env::var("ZEN_UI_SCREENSHOT_DELAY").ok().and_then(|value| value.parse().ok()).unwrap_or(SETTLE_SECONDS);
    let frame = ctx.cumulative_frame_nr();
    let settled = frame >= SETTLE_FRAMES && ctx.input(|input| input.time) >= delay;
    if !settled || REQUESTED.load(std::sync::atomic::Ordering::Relaxed) {
        ctx.request_repaint();
        return;
    }
    REQUESTED.store(true, std::sync::atomic::Ordering::Relaxed);
    let callback = egui::ScreenshotCallback::new(move |image| {
        if let Err(error) = crate::offscreen::write_png(&target, &image) {
            #[allow(clippy::print_stderr, reason = "a developer tool, run from a terminal")]
            {
                eprintln!("cannot write {}: {error}", target.display());
            }
        }
        if let Some(hook) = BEFORE_QUIT.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref() {
            hook();
        }
        std::process::exit(0);
    });
    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(callback));
    ctx.request_repaint();
}
