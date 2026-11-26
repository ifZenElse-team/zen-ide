use std::ops::Range;

use zen_text::Edit;

use super::expansion::Expanded;
use super::expressions::{self, Place};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateSession {
    template: String,
    variables: Vec<(String, Vec<Range<usize>>)>,
    order: Vec<usize>,
    dependents: Vec<(usize, String)>,
    place: Place,
    end: usize,
    current: usize,
}

impl TemplateSession {
    pub(crate) fn new(template: &str, expanded: &Expanded, at: usize) -> Option<Self> {
        let variables: Vec<(String, Vec<Range<usize>>)> = expanded
            .variables
            .iter()
            .map(|(name, ranges)| (name.clone(), ranges.iter().map(|range| range.start + at..range.end + at).collect()))
            .collect();
        let order: Vec<usize> =
            (0..variables.len()).filter(|index| !expanded.skipped.contains(&variables[*index].0)).collect();
        if order.is_empty() {
            return None;
        }
        let dependents = expanded
            .dependents
            .iter()
            .filter_map(|(name, expression)| {
                Some((variables.iter().position(|(known, _)| known == name)?, expression.clone()))
            })
            .collect();
        Some(Self {
            template: template.to_owned(),
            variables,
            order,
            dependents,
            place: expanded.place.clone(),
            end: expanded.end + at,
            current: 0,
        })
    }

    pub fn template(&self) -> &str {
        &self.template
    }

    pub fn current(&self) -> (usize, &str) {
        (self.current, &self.variables[self.order[self.current]].0)
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn current_ranges(&self) -> &[Range<usize>] {
        &self.variables[self.order[self.current]].1
    }

    pub fn ranges(&self) -> impl Iterator<Item = (&Range<usize>, bool)> {
        let current = self.order[self.current];
        self.variables
            .iter()
            .enumerate()
            .filter(|(index, _)| self.order.contains(index))
            .flat_map(move |(index, (_, ranges))| ranges.iter().map(move |range| (range, index == current)))
    }

    pub fn end(&self) -> usize {
        self.end
    }

    pub fn contains(&self, offset: usize) -> bool {
        let starts = self.variables.iter().flat_map(|(_, ranges)| ranges.iter().map(|range| range.start));
        let ends = self.variables.iter().flat_map(|(_, ranges)| ranges.iter().map(|range| range.end));
        let start = starts.chain([self.end]).min().unwrap_or(self.end);
        let end = ends.chain([self.end]).max().unwrap_or(self.end);
        (start..=end).contains(&offset)
    }

    pub(crate) fn go_to(&mut self, index: usize) -> bool {
        if index < self.order.len() {
            self.current = index;
            true
        } else {
            false
        }
    }

    pub(crate) fn dependent_values(&self, text: &str) -> Vec<(Range<usize>, String)> {
        let chars: Vec<char> = text.chars().collect();
        let value_of = |index: usize| -> Option<String> {
            let range = self.variables.get(index)?.1.first()?;
            Some(chars.get(range.clone())?.iter().collect())
        };
        let current = self.order[self.current];
        let visited: Vec<usize> = self.order[..=self.current].to_vec();
        let mut replacements = Vec::new();
        for (index, expression) in &self.dependents {
            if *index == current || visited.contains(index) {
                continue;
            }
            let values = |name: &str| {
                let found = self.variables.iter().position(|(known, _)| known == name)?;
                value_of(found)
            };
            let value = expressions::evaluate(expression, &values, &self.place);
            if value.is_empty() || value_of(*index).as_deref() == Some(value.as_str()) {
                continue;
            }
            replacements.extend(self.variables[*index].1.iter().map(|range| (range.clone(), value.clone())));
        }
        replacements.sort_by_key(|(range, _)| range.start);
        replacements
    }

    pub(crate) fn replaced(&mut self, replacements: &[(Range<usize>, String)]) {
        let shifted = |offset: usize, is_end: bool| -> usize {
            let mut moved = offset as isize;
            for (range, text) in replacements {
                let after = range.end < offset || (range.end == offset && (is_end || !range.is_empty()));
                if after {
                    moved += text.chars().count() as isize - range.len() as isize;
                }
            }
            moved.max(0) as usize
        };
        for (_, ranges) in &mut self.variables {
            for range in ranges.iter_mut() {
                *range = shifted(range.start, false)..shifted(range.end, true);
            }
        }
        self.end = shifted(self.end, true);
    }

    pub(crate) fn map(&mut self, edit: &Edit) -> bool {
        let replaced = edit.range.clone();
        let inserted = edit.text.chars().count();
        let shift = |offset: usize| (offset + inserted).saturating_sub(replaced.len());
        let current = self.order[self.current];
        let at_current_start =
            replaced.is_empty() && self.variables[current].1.iter().any(|range| range.start == replaced.start);
        for (index, (_, ranges)) in self.variables.iter_mut().enumerate() {
            for range in ranges.iter_mut() {
                let inside = replaced.start >= range.start && replaced.end <= range.end;
                if index == current && inside {
                    range.end = shift(range.end);
                } else if replaced.end <= range.start {
                    *range = shift(range.start)..shift(range.end);
                } else if replaced.start < range.end {
                    if !inside {
                        return false;
                    }
                    range.end = shift(range.end);
                }
            }
        }
        if replaced.is_empty() && replaced.start == self.end {
            if !at_current_start {
                self.end = shift(self.end);
            }
        } else if replaced.end <= self.end {
            self.end = shift(self.end);
        } else if replaced.start < self.end {
            self.end = replaced.start + inserted;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> TemplateSession {
        let expanded = Expanded {
            text: "f(a, a)\n".to_owned(),
            variables: vec![("A".to_owned(), vec![2..3, 5..6]), ("B".to_owned(), std::iter::once(7..7).collect())],
            end: 8,
            ..Expanded::default()
        };
        TemplateSession::new("f", &expanded, 10).unwrap()
    }

    #[test]
    fn typing_grows_the_current_variable_and_moves_the_rest() {
        let mut session = session();
        assert!(session.map(&Edit::replace(15..16, "xy")));
        assert!(session.map(&Edit::replace(12..13, "xy")));
        assert_eq!(session.current_ranges(), [12..14, 16..18]);
        assert!(session.map(&Edit::replace(14..14, "z")));
        assert_eq!(session.current_ranges(), [12..15, 17..19]);
        assert_eq!(session.variables[1].1, std::slice::from_ref(&(20..20)));
        assert_eq!(session.end(), 21);
        assert!(session.map(&Edit::replace(0..0, "//")));
        assert_eq!(session.current_ranges()[0], 14..17);
        assert!(session.contains(14) && session.contains(22) && !session.contains(13));
    }

    #[test]
    fn an_edit_across_a_variable_ends_the_session() {
        let mut broken = session();
        assert!(!broken.map(&Edit::replace(11..13, "")));
        let mut session = session();
        assert!(session.go_to(1) && !session.go_to(2));
        assert!(session.map(&Edit::replace(17..17, "q")));
        assert_eq!(session.current_ranges(), std::slice::from_ref(&(17..18)), "the empty variable takes what is typed");
        assert_eq!(session.end(), 19);
    }

    #[test]
    fn a_dependent_variable_follows_the_one_it_is_computed_from_and_tab_skips_a_defined_one() {
        let expanded = Expanded {
            text: "T n n x".to_owned(),
            variables: vec![
                ("TYPE".to_owned(), std::iter::once(0..1).collect()),
                ("NAME".to_owned(), vec![2..3, 4..5]),
                ("X".to_owned(), std::iter::once(6..7).collect()),
            ],
            end: 7,
            skipped: vec!["X".to_owned()],
            dependents: vec![("NAME".to_owned(), "decapitalize(TYPE)".to_owned())],
            place: Place::default(),
        };
        let mut session = TemplateSession::new("t", &expanded, 0).unwrap();
        assert_eq!(session.len(), 2, "the defined X is not visited");
        assert!(session.map(&Edit::replace(0..1, "Server")));
        let replacements = session.dependent_values("Server n n x");
        assert_eq!(replacements, vec![(7..8, "server".to_owned()), (9..10, "server".to_owned())]);
        session.replaced(&replacements);
        assert_eq!(session.variables[1].1, vec![7..13, 14..20]);
        assert_eq!((session.variables[2].1.clone(), session.end()), (std::iter::once(21..22).collect::<Vec<_>>(), 22));
        assert!(session.go_to(1));
        assert!(session.dependent_values("Server server server x").is_empty(), "a visited one is the user's");
    }
}
