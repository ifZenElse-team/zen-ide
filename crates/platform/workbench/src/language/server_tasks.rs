use std::collections::BTreeMap;

use zen_core::Session;
use zen_i18n::t;

use super::{Languages, WorkProgress};
use crate::tasks::{TaskHandle, Tasks};

type Key = (String, String);

#[derive(Default)]
struct ServerTasks {
    tasks: BTreeMap<Key, (TaskHandle, WorkProgress)>,
}

fn title(work: &WorkProgress) -> String {
    t!("workbench-language-task", server = work.server.as_str(), title = work.title.as_str())
}

fn show(task: &TaskHandle, work: &WorkProgress) {
    let fraction = work.percentage.map(|percentage| percentage as f32 / 100.0);
    task.progress(fraction, work.message.as_deref().unwrap_or_default());
    task.set_cancellable(work.cancellable);
}

pub(crate) fn publish(session: &mut Session) {
    let works: BTreeMap<Key, WorkProgress> = session
        .state::<Languages>()
        .map(Languages::works)
        .unwrap_or_default()
        .into_iter()
        .map(|(token, work)| ((work.server.clone(), token), work))
        .collect();
    if works.is_empty() && session.state::<ServerTasks>().is_none_or(|known| known.tasks.is_empty()) {
        return;
    }
    let mut known = std::mem::take(session.state_mut::<ServerTasks>());
    known.tasks.retain(|key, (task, _)| {
        let goes_on = works.contains_key(key);
        if !goes_on {
            task.dismiss();
        }
        goes_on
    });
    for (key, work) in works {
        if let Some((task, shown)) = known.tasks.get_mut(&key) {
            if *shown != work {
                show(task, &work);
                *shown = work;
            }
            continue;
        }
        let task = session.state_mut::<Tasks>().start(&title(&work), crate::exit::LANGUAGE_SERVER, false);
        let canceller = session.state::<Languages>().and_then(|languages| languages.work_canceller(&key.0, &key.1));
        if let Some(canceller) = canceller {
            task.on_cancel(canceller);
        }
        show(&task, &work);
        known.tasks.insert(key, (task, work));
    }
    session.set_state(known);
}
