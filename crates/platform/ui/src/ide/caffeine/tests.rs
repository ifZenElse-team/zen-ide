use super::sprite::{self, Badge, Grid, Ink, Look};
use super::*;

fn status(on: bool, awake: bool) -> CaffeineStatus {
    zen_i18n::register(crate::toolbox::LOCALES);
    CaffeineStatus {
        available: true,
        backend: Some("caffeinate".to_owned()),
        on,
        awake,
        what: vec![Assertion::Idle],
        length: on.then_some(Length::Indefinitely),
        seconds_left: None,
        during: Vec::new(),
        busy: Vec::new(),
        paused: None,
        ended: None,
        battery: None,
        pid: None,
        error: None,
        maximum: false,
    }
}

fn inks<const N: usize>(grid: &Grid<N>) -> std::collections::HashSet<Ink> {
    grid.iter().flatten().copied().collect()
}

#[test]
fn each_state_of_caffeine_has_its_look() {
    assert_eq!(look(&status(false, false)), Look::Asleep, "off: asleep");
    assert_eq!(look(&CaffeineStatus { available: false, ..status(false, false) }), Look::Asleep);
    assert_eq!(look(&status(true, false)), Look::Waiting, "paused: waiting with the cup down");
    assert_eq!(look(&status(true, true)), Look::Steady, "-i: sipping steadily");
    let mut display = status(true, true);
    display.what = vec![Assertion::Idle, Assertion::Display];
    assert_eq!(look(&display), Look::Wide, "-d: eyes wide open");
    display.what = vec![Assertion::Display, Assertion::Disk, Assertion::User];
    assert_eq!(look(&display), Look::Wide, "not every option: still only wide awake");
    let mut every = status(true, true);
    every.what = Assertion::ALL.to_vec();
    assert_eq!(look(&every), Look::Wired, "-i -d -m -s -u: red eyes, gulping");
    every.awake = false;
    assert_eq!(look(&every), Look::Waiting, "paused wins over the options");
}

#[test]
fn every_look_is_drawn_as_its_state_says() {
    for look in Look::ALL {
        assert!(sprite::frames(look) >= 3, "{look:?} moves");
        let all: Vec<_> = (0..sprite::frames(look)).map(|index| inks(&sprite::frame(look, index))).collect();
        let any = |ink: Ink| all.iter().any(|frame| frame.contains(&ink));
        let every = |ink: Ink| all.iter().all(|frame| frame.contains(&ink));
        assert!(every(Ink::Skin) && every(Ink::Shirt) && every(Ink::Cup) && every(Ink::Outline), "{look:?}");
        assert_eq!(every(Ink::Red), look == Look::Wired, "{look:?}: only the most awake has bloodshot eyes");
        assert_eq!(any(Ink::Red), look == Look::Wired, "{look:?}");
        assert_eq!(any(Ink::Snore), look == Look::Asleep, "{look:?}: zZ only asleep");
        assert_eq!(any(Ink::Steam), matches!(look, Look::Steady | Look::Wide | Look::Wired), "{look:?}: hot coffee");
        assert_eq!(any(Ink::White), look == Look::Wide, "{look:?}: eyes wide open");
    }
    for look in Look::ALL {
        let first = sprite::frame(look, 0);
        assert!((1..sprite::frames(look)).any(|index| sprite::frame(look, index) != first), "{look:?}");
    }
}

#[test]
fn the_more_awake_the_more_often_he_drinks() {
    let sips = |look: Look| sprite::cycle_ms(look);
    assert!(sips(Look::Wired) < sips(Look::Wide) && sips(Look::Wide) < sips(Look::Steady));
    assert!(sips(Look::Wired) <= 1000, "gulping very often: {} ms", sips(Look::Wired));
    for look in Look::ALL {
        let cycle = sprite::cycle_ms(look);
        let (index, left) = sprite::frame_at(look, 0);
        assert_eq!(index, 0);
        assert!(left > 0);
        assert_eq!(sprite::frame_at(look, cycle), sprite::frame_at(look, 0), "it loops");
        let (last, left) = sprite::frame_at(look, cycle - 1);
        assert_eq!((last, left), (sprite::frames(look) - 1, 1));
    }
}

#[test]
fn only_a_shown_tool_window_moves_and_reduce_motion_stills_it() {
    assert!(animating(true, false));
    assert!(!animating(false, false), "a hidden window costs no frames");
    assert!(!animating(true, true), "Reduce motion: a still frame");
    for look in Look::ALL {
        let (index, next) = frame_now(look, 2.3, false);
        assert_eq!((index, next), (sprite::still(look), None), "{look:?}: still, no repaint");
        assert_eq!(frame_now(look, 7.9, false), (index, None), "the still frame does not change with time");
        let (_, next) = frame_now(look, 2.3, true);
        assert!(next.is_some_and(|next| next <= Duration::from_secs(2)), "{look:?}: the next frame is asked for");
    }
}

#[test]
fn the_stripe_cup_shows_the_state() {
    assert_eq!(Badge::of(Look::Asleep), Badge::Asleep);
    assert_eq!(Badge::of(Look::Waiting), Badge::Cold);
    assert_eq!(Badge::of(Look::Steady), Badge::Hot);
    assert_eq!(Badge::of(Look::Wide), Badge::Hot);
    assert_eq!(Badge::of(Look::Wired), Badge::Max);
    let asleep = inks(&sprite::icon(Badge::Asleep));
    let cold = inks(&sprite::icon(Badge::Cold));
    let hot = inks(&sprite::icon(Badge::Hot));
    let max = inks(&sprite::icon(Badge::Max));
    assert!(asleep.contains(&Ink::Snore) && !asleep.contains(&Ink::Steam));
    assert!(!cold.contains(&Ink::Snore) && !cold.contains(&Ink::Steam));
    assert!(hot.contains(&Ink::Steam) && !hot.contains(&Ink::Red));
    assert!(max.contains(&Ink::Red), "maximum: red steam");
}

#[test]
fn pixels_are_whole_physical_pixels() {
    for ppp in [1.0, 1.5, 2.0, 3.0] {
        for room in [64.0, 150.0, 224.0, 15.0] {
            let pixel = pixel_size(room, sprite::SIZE, ppp);
            let physical = pixel * ppp;
            assert!((physical - physical.round()).abs() < 1e-4 && physical >= 1.0, "{room} at {ppp}: {pixel}");
            assert!(pixel * sprite::SIZE as f32 <= room.max(sprite::SIZE as f32 / ppp) + 1e-3);
        }
    }
    assert_eq!(pixel_size(15.0, sprite::ICON, 2.0), 1.0, "the stripe cup: a point a pixel");
}

#[test]
fn every_look_has_colours_from_the_theme_and_a_fixed_red() {
    for palette in [IdePalette::zen_dark(), IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_light())] {
        assert_eq!(paint(&palette, Ink::Clear), None);
        assert_eq!(paint(&palette, Ink::Red), Some(BLOODSHOT));
        assert_eq!(paint(&palette, Ink::Shirt), Some(palette.accent));
        let skin = paint(&palette, Ink::Skin).unwrap();
        let pupil = paint(&palette, Ink::Pupil).unwrap();
        assert_ne!(skin, pupil);
        assert_ne!(paint(&palette, Ink::White), paint(&palette, Ink::Pupil), "the whites of the eyes show");
    }
}

fn in_english(test: impl FnOnce()) {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::with_language("en", test);
}

#[test]
fn the_time_left_is_short() {
    in_english(the_time_left_is_short_in_english);
}

fn the_time_left_is_short_in_english() {
    assert_eq!(time_left(42 * 60), t!("ide-caffeine-left-minutes", minutes = 42));
    assert_eq!(time_left(1), time_left(60), "a started minute counts");
    assert_eq!(time_left(90 * 60), t!("ide-caffeine-left-hours", hours = 1, minutes = 30));
    assert_eq!(time_left(120 * 60), t!("ide-caffeine-left-whole-hours", hours = 2));
    assert_eq!(time_left(60 * 60), t!("ide-caffeine-left-whole-hours", hours = 1), "1 h, not 1 h 0 min");
    assert_eq!(time_left(240 * 60), t!("ide-caffeine-left-whole-hours", hours = 4));
    assert_eq!(time_left(61 * 60), t!("ide-caffeine-left-hours", hours = 1, minutes = 1));
    for (source, hours) in [
        (include_str!("../../../locales/en/caffeine.ftl"), "ide-caffeine-left-whole-hours = { $hours } h\n"),
        (include_str!("../../../locales/ru/caffeine.ftl"), "ide-caffeine-left-whole-hours = { $hours } ч\n"),
    ] {
        assert!(source.contains(hours), "{hours}");
    }
}

#[test]
fn the_description_says_what_is_kept_awake_and_why_not() {
    in_english(the_description_in_english);
}

fn the_description_in_english() {
    let off = describe(&status(false, false), &CaffeineSettings::default());
    assert!(off.contains(&t!("ide-caffeine-off")));
    let mut paused = status(true, false);
    paused.paused = Some(Pause::OnBattery);
    assert!(describe(&paused, &CaffeineSettings::default()).contains(&t!("ide-caffeine-paused-battery")));
    let mut display = status(true, true);
    display.what = vec![Assertion::Display];
    display.seconds_left = Some(600);
    display.length = Some(Length::Minutes { minutes: 10 });
    let hint = describe(&display, &CaffeineSettings::default());
    assert!(hint.contains(&t!("ide-caffeine-lid")), "{hint}");
    let unavailable = CaffeineStatus { available: false, ..status(false, false) };
    assert_eq!(describe(&unavailable, &CaffeineSettings::default()), t!("ide-caffeine-unavailable"));
    for look in Look::ALL {
        assert!(!headline(look).is_empty());
    }
}

#[test]
fn the_summary_is_two_short_lines_and_the_whole_story_is_its_hint() {
    in_english(the_summary_in_english);
}

fn the_summary_in_english() {
    let settings = CaffeineSettings::default();
    let mut every = status(true, true);
    every.what = Assertion::ALL.to_vec();
    let (first, second) = summary(&every);
    assert_eq!(
        first,
        t!("ide-caffeine-summary-on", what = t!("ide-caffeine-short-everything"), length = t!("ide-caffeine-for-ever"))
    );
    assert!(second.is_empty());
    assert!(describe(&every, &settings).contains(&t!("ide-caffeine-lid")), "the lid rule is in the hint");
    let mut timed = status(true, true);
    timed.what = vec![Assertion::Idle, Assertion::Display];
    timed.length = Some(Length::Minutes { minutes: 90 });
    timed.seconds_left = Some(90 * 60);
    let (first, _) = summary(&timed);
    assert!(first.contains(&time_left(90 * 60)) && first.contains(&t!("ide-caffeine-short-display")), "{first}");
    let mut waiting = status(true, false);
    waiting.length = Some(Length::While);
    waiting.paused = Some(Pause::Waiting);
    assert_eq!(summary(&waiting).1, t!("ide-caffeine-paused-waiting"));
    let mut failed = status(true, false);
    failed.error = Some("no such file: /usr/bin/caffeinate and a long story".to_owned());
    assert_eq!(summary(&failed).1, t!("ide-caffeine-failed-short"));
    assert_eq!(summary(&status(false, false)), (t!("ide-caffeine-summary-off"), String::new()));
    let unavailable = CaffeineStatus { available: false, ..status(false, false) };
    assert_eq!(summary(&unavailable).0, t!("ide-caffeine-unavailable-short"));
    for language in ["en", "ru"] {
        zen_i18n::with_language(language, || {
            for status in [&every, &timed, &waiting, &failed, &unavailable] {
                let (first, second) = summary(status);
                assert!(!first.contains('\n') && !second.contains('\n'), "{language}: {first} / {second}");
                assert!(first.chars().count() <= 60 && second.chars().count() <= 60, "{language}: {first} / {second}");
            }
        });
    }
}
