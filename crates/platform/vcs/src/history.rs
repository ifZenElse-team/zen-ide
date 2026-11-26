use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Oid(pub [u8; 20]);

impl Oid {
    pub fn from_hex(hex: &str) -> Option<Oid> {
        let bytes = hex.as_bytes();
        if bytes.len() != 40 {
            return None;
        }
        let mut id = [0u8; 20];
        for (at, pair) in bytes.chunks(2).enumerate() {
            id[at] = (nibble(pair[0])? << 4) | nibble(pair[1])?;
        }
        Some(Oid(id))
    }

    pub fn to_hex(&self) -> String {
        let mut hex = String::with_capacity(40);
        for byte in self.0 {
            hex.push(char::from(HEX[usize::from(byte >> 4)]));
            hex.push(char::from(HEX[usize::from(byte & 15)]));
        }
        hex
    }

    pub fn starts_with_hex(&self, prefix: &str) -> bool {
        let prefix = prefix.as_bytes();
        prefix.len() <= 40
            && prefix.iter().enumerate().all(|(at, &digit)| {
                let byte = self.0[at / 2];
                let half = if at % 2 == 0 { byte >> 4 } else { byte & 15 };
                HEX[usize::from(half)] == digit
            })
    }
}

const HEX: &[u8; 16] = b"0123456789abcdef";

fn nibble(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CommitRecord<'a> {
    pub id: Oid,
    pub parents: &'a [Oid],
    pub author: &'a str,
    pub email: &'a str,
    pub time: i64,
    pub offset: i32,
    pub subject: &'a str,
    pub body: &'a str,
}

#[derive(Clone, Copy, Debug)]
struct Fields {
    time: i64,
    offset: i32,
    person: u32,
    parents: u32,
    text: u32,
    subject: u32,
}

#[derive(Default)]
pub struct HistoryStore {
    ids: Vec<Oid>,
    fields: Vec<Fields>,
    parents: Vec<Oid>,
    text: String,
    people: Vec<Box<str>>,
    person_of: HashMap<Box<str>, u32>,
    scratch: String,
    index: HashMap<Oid, u32>,
    out_of_order: bool,
}

impl HistoryStore {
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn push(&mut self, record: &CommitRecord<'_>) -> usize {
        if let Some(&at) = self.index.get(&record.id) {
            return at as usize;
        }
        let at = self.ids.len();
        if record.parents.iter().any(|parent| self.index.contains_key(parent)) {
            self.out_of_order = true;
        }
        self.scratch.clear();
        self.scratch.push_str(record.author);
        self.scratch.push('\0');
        self.scratch.push_str(record.email);
        let person = match self.person_of.get(self.scratch.as_str()) {
            Some(&person) => person,
            None => {
                let person = self.people.len() as u32;
                let key: Box<str> = self.scratch.as_str().into();
                self.people.push(key.clone());
                self.person_of.insert(key, person);
                person
            }
        };
        let room = (u32::MAX as usize).saturating_sub(self.text.len());
        let subject = clip(record.subject, room);
        let body = clip(record.body, room - subject.len());
        let text = self.text.len() as u32;
        self.text.push_str(subject);
        self.text.push_str(body);
        self.fields.push(Fields {
            time: record.time,
            offset: record.offset,
            person,
            parents: self.parents.len() as u32,
            text,
            subject: subject.len() as u32,
        });
        self.parents.extend_from_slice(record.parents);
        self.ids.push(record.id);
        self.index.insert(record.id, at as u32);
        at
    }

    pub fn record(&self, at: usize) -> CommitRecord<'_> {
        let (author, email) = self.author(at);
        let (time, offset) = self.time(at);
        CommitRecord {
            id: self.ids[at],
            parents: self.parents(at),
            author,
            email,
            time,
            offset,
            subject: self.subject(at),
            body: self.body(at),
        }
    }

    pub fn clear(&mut self) {
        self.ids.clear();
        self.fields.clear();
        self.parents.clear();
        self.text.clear();
        self.index.clear();
        self.out_of_order = false;
    }

    pub fn index_of(&self, id: &Oid) -> Option<usize> {
        self.index.get(id).map(|&at| at as usize)
    }

    pub fn id(&self, at: usize) -> Oid {
        self.ids[at]
    }

    pub fn parents(&self, at: usize) -> &[Oid] {
        let start = self.fields[at].parents as usize;
        let end = self.fields.get(at + 1).map_or(self.parents.len(), |next| next.parents as usize);
        &self.parents[start..end]
    }

    pub fn author(&self, at: usize) -> (&str, &str) {
        let person = &self.people[self.fields[at].person as usize];
        person.split_once('\0').unwrap_or((person, ""))
    }

    pub fn time(&self, at: usize) -> (i64, i32) {
        (self.fields[at].time, self.fields[at].offset)
    }

    pub fn subject(&self, at: usize) -> &str {
        let fields = self.fields[at];
        &self.text[fields.text as usize..(fields.text + fields.subject) as usize]
    }

    pub fn body(&self, at: usize) -> &str {
        let fields = self.fields[at];
        let end = self.fields.get(at + 1).map_or(self.text.len(), |next| next.text as usize);
        &self.text[(fields.text + fields.subject) as usize..end]
    }

    pub fn out_of_order(&self) -> bool {
        self.out_of_order
    }

    pub fn children_first(&self) -> Vec<usize> {
        children_first(self.len(), |at| self.parents(at).iter().filter_map(|parent| self.index_of(parent)))
    }

    pub fn ancestors(&self, target: usize) -> Vec<bool> {
        let mut reached = vec![false; self.len()];
        let mut todo = Vec::new();
        if target < self.len() {
            todo.push(target);
        }
        while let Some(at) = todo.pop() {
            if std::mem::replace(&mut reached[at], true) {
                continue;
            }
            todo.extend(self.parents(at).iter().filter_map(|parent| self.index_of(parent)).filter(|&p| !reached[p]));
        }
        reached
    }

    pub fn distances_to(&self, target: usize, order: Option<&[u32]>) -> Vec<u32> {
        let mut distance = vec![u32::MAX; self.len()];
        let Some(slot) = distance.get_mut(target) else { return distance };
        *slot = 0;
        let before: Box<dyn Iterator<Item = usize>> = match order {
            Some(order) => {
                let at = order.iter().position(|&at| at as usize == target).unwrap_or(0);
                Box::new(order[..at].iter().rev().map(|&at| at as usize))
            }
            None => Box::new((0..target).rev()),
        };
        for at in before {
            let nearest = self
                .parents(at)
                .iter()
                .filter_map(|parent| self.index_of(parent))
                .map(|parent| distance[parent])
                .min()
                .unwrap_or(u32::MAX);
            distance[at] = nearest.saturating_add(1);
        }
        distance
    }

    pub fn descendants(&self, target: usize, order: Option<&[u32]>) -> Vec<bool> {
        let mut reaches = vec![false; self.len()];
        let Some(slot) = reaches.get_mut(target) else { return reaches };
        *slot = true;
        let before: Box<dyn Iterator<Item = usize>> = match order {
            Some(order) => {
                let at = order.iter().position(|&at| at as usize == target).unwrap_or(0);
                Box::new(order[..at].iter().rev().map(|&at| at as usize))
            }
            None => Box::new((0..target).rev()),
        };
        for at in before {
            reaches[at] =
                self.parents(at).iter().any(|parent| self.index_of(parent).is_some_and(|parent| reaches[parent]));
        }
        reaches
    }

    pub fn heap_bytes(&self) -> usize {
        use std::mem::size_of;
        let people: usize = self.people.iter().map(|person| person.len()).sum();
        self.ids.capacity() * size_of::<Oid>()
            + self.fields.capacity() * size_of::<Fields>()
            + self.parents.capacity() * size_of::<Oid>()
            + self.text.capacity()
            + self.people.capacity() * size_of::<Box<str>>()
            + people * 2
            + self.scratch.capacity()
            + map_bytes(self.person_of.capacity(), size_of::<(Box<str>, u32)>())
            + map_bytes(self.index.capacity(), size_of::<(Oid, u32)>())
    }
}

pub fn map_bytes(capacity: usize, entry: usize) -> usize {
    if capacity == 0 {
        return 0;
    }
    let buckets = (capacity * 8 / 7).next_power_of_two();
    buckets * (entry + 1)
}

fn clip(text: &str, room: usize) -> &str {
    if text.len() <= room {
        return text;
    }
    let mut end = room;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

pub fn children_first<P: Iterator<Item = usize>>(count: usize, parents: impl Fn(usize) -> P) -> Vec<usize> {
    let mut waiting = vec![0usize; count];
    for at in 0..count {
        for parent in parents(at) {
            waiting[parent] += 1;
        }
    }
    let mut ready: std::collections::BinaryHeap<std::cmp::Reverse<usize>> =
        (0..count).filter(|&at| waiting[at] == 0).map(std::cmp::Reverse).collect();
    let mut taken = vec![false; count];
    let mut order = Vec::with_capacity(count);
    while order.len() < count {
        let Some(std::cmp::Reverse(next)) = ready.pop() else {
            order.extend((0..count).filter(|&at| !taken[at]));
            break;
        };
        if std::mem::replace(&mut taken[next], true) {
            continue;
        }
        for parent in parents(next) {
            waiting[parent] = waiting[parent].saturating_sub(1);
            if waiting[parent] == 0 && !taken[parent] {
                ready.push(std::cmp::Reverse(parent));
            }
        }
        order.push(next);
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(byte: u8) -> Oid {
        Oid([byte; 20])
    }

    #[test]
    fn ids_go_to_hex_and_back() {
        let id = Oid::from_hex("0123456789abcdef0123456789ABCDEF01234567").unwrap();
        assert_eq!(id.to_hex(), "0123456789abcdef0123456789abcdef01234567");
        assert!(id.starts_with_hex("0123") && id.starts_with_hex("") && !id.starts_with_hex("1"));
        assert!(id.starts_with_hex(&id.to_hex()));
        assert_eq!(Oid::from_hex("0123"), None);
        assert_eq!(Oid::from_hex(&"g".repeat(40)), None);
    }

    #[test]
    fn the_store_gives_back_what_was_added() {
        let mut store = HistoryStore::default();
        let record = |id, parents: &'static [Oid], author, subject, body| CommitRecord {
            id,
            parents,
            author,
            email: "a@example.com",
            time: 10,
            offset: 3600,
            subject,
            body,
        };
        const PARENTS: &[Oid] = &[Oid([2; 20]), Oid([3; 20])];
        assert_eq!(store.push(&record(oid(1), PARENTS, "Ada", "Merge", "Both lines.")), 0);
        assert_eq!(store.push(&record(oid(2), &[], "Привет", "Second", "")), 1);
        assert_eq!(store.push(&record(oid(3), &[], "Ada", "Third", "Body\nmore")), 2);
        assert_eq!(store.push(&record(oid(1), &[], "Ada", "Again", "")), 0, "a commit held is not added again");
        assert_eq!(store.len(), 3);
        assert_eq!(store.parents(0), PARENTS);
        assert!(store.parents(1).is_empty() && store.parents(2).is_empty());
        assert_eq!((store.subject(0), store.body(0)), ("Merge", "Both lines."));
        assert_eq!((store.subject(1), store.body(1)), ("Second", ""));
        assert_eq!((store.subject(2), store.body(2)), ("Third", "Body\nmore"));
        assert_eq!(store.author(1), ("Привет", "a@example.com"));
        assert_eq!(store.author(2), store.author(0));
        assert_eq!(store.people.len(), 2, "each author once");
        assert_eq!(store.time(2), (10, 3600));
        assert_eq!(store.index_of(&oid(3)), Some(2));
        assert_eq!(store.index_of(&oid(9)), None);
        assert!(!store.out_of_order());
        assert_eq!(store.children_first(), [0, 1, 2]);
        assert!(store.heap_bytes() > 0);
    }

    #[test]
    fn a_parent_before_its_child_is_put_after_it() {
        let mut store = HistoryStore::default();
        let record = |id, parents: &'static [Oid]| CommitRecord {
            id,
            parents,
            author: "",
            email: "",
            time: 0,
            offset: 0,
            subject: "",
            body: "",
        };
        store.push(&record(oid(3), &[Oid([1; 20])]));
        store.push(&record(oid(1), &[]));
        store.push(&record(oid(2), &[Oid([1; 20])]));
        assert!(store.out_of_order());
        assert_eq!(store.children_first(), [0, 2, 1]);
        let order: Vec<u32> = store.children_first().into_iter().map(|at| at as u32).collect();
        assert_eq!(store.descendants(1, Some(&order)), [true, true, true]);
        assert_eq!(store.descendants(2, Some(&order)), [false, false, true]);
    }

    #[test]
    fn descendants_are_the_commits_that_reach_one() {
        let mut store = HistoryStore::default();
        let [m, f, b, y, a] = [1u8, 2, 3, 4, 5].map(oid);
        let record = |id, parents: &[Oid]| {
            let parents = parents.to_vec();
            (id, parents)
        };
        for (id, parents) in [record(m, &[b, f]), record(f, &[a]), record(b, &[a]), record(y, &[a]), record(a, &[])] {
            store.push(&CommitRecord {
                id,
                parents: &parents,
                author: "",
                email: "",
                time: 0,
                offset: 0,
                subject: "",
                body: "",
            });
        }
        assert_eq!(store.descendants(1, None), [true, true, false, false, false], "f is in m's history");
        assert_eq!(store.descendants(4, None), [true; 5], "a is in every history");
        assert_eq!(store.descendants(3, None), [false, false, false, true, false]);
        assert!(store.descendants(9, None).iter().all(|reaches| !reaches), "a commit not held");
        assert_eq!(store.ancestors(0), [true, true, true, false, true], "m's history: f, b and a, not y");
        assert_eq!(store.ancestors(3), [false, false, false, true, true]);
        assert!(store.ancestors(9).iter().all(|reached| !reached));
        assert_eq!(store.distances_to(4, None), [2, 1, 1, 1, 0], "m is two steps above a");
        assert_eq!(store.distances_to(1, None), [1, 0, u32::MAX, u32::MAX, u32::MAX]);
    }

    #[test]
    fn a_message_past_the_room_left_is_cut_at_a_character() {
        assert_eq!(clip("héllo", 10), "héllo");
        assert_eq!(clip("héllo", 2), "h");
        assert_eq!(clip("héllo", 3), "hé");
        assert_eq!(clip("héllo", 0), "");
    }
}
