use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{CommandLine, Configuration, RunContext};

/// What a run records.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ProfileKind {
    /// Where the program spends processor time.
    #[default]
    Cpu,
    /// Where the program allocates memory.
    Memory,
}

/// One thing a profile measures per sample.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileMetric {
    /// Its name, such as `cpu` or `alloc_space`.
    pub name: String,
    /// Its unit, such as `nanoseconds` or `bytes`.
    pub unit: String,
}

/// A function that appears in a profile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileFrame {
    /// The function's full name.
    pub function: String,
    /// Its source file, when the profile names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
    /// The line it starts at, from 1; 0 when unknown.
    pub line: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileSample {
    pub stack: Vec<u32>,
    pub values: Vec<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Profile {
    pub metrics: Vec<ProfileMetric>,
    pub default_metric: usize,
    pub frames: Vec<ProfileFrame>,
    pub samples: Vec<ProfileSample>,
    pub duration_ns: u64,
}

/// What one function costs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FunctionCost {
    /// The frame's index, for `profile.frame.open`.
    pub frame: u32,
    /// What the function itself cost.
    pub flat: i64,
    /// What it cost with everything it called.
    pub cumulative: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CallNode {
    pub frame: Option<u32>,
    pub value: i64,
    pub children: Vec<CallNode>,
    pub omitted: usize,
}

#[derive(Default)]
struct TreeNode {
    frame: Option<u32>,
    value: i64,
    children: std::collections::BTreeMap<u32, usize>,
}

fn pruned(nodes: &[TreeNode], index: usize, depth: usize, least: i64) -> CallNode {
    let node = &nodes[index];
    let mut children: Vec<usize> = node.children.values().copied().collect();
    let all = children.len();
    children.retain(|child| depth > 0 && nodes[*child].value >= least);
    children.sort_by_key(|child| std::cmp::Reverse(nodes[*child].value));
    CallNode {
        frame: node.frame,
        value: node.value,
        omitted: all - children.len(),
        children: children.into_iter().map(|child| pruned(nodes, child, depth - 1, least)).collect(),
    }
}

impl Profile {
    pub fn metric_index(&self, name: &str) -> Option<usize> {
        self.metrics.iter().position(|metric| metric.name == name)
    }

    pub fn total(&self, metric: usize) -> i64 {
        self.samples.iter().filter_map(|sample| sample.values.get(metric)).sum()
    }

    pub fn functions(&self, metric: usize) -> Vec<FunctionCost> {
        let mut costs: Vec<FunctionCost> =
            (0..self.frames.len()).map(|frame| FunctionCost { frame: frame as u32, flat: 0, cumulative: 0 }).collect();
        let mut counted = vec![usize::MAX; costs.len()];
        for (number, sample) in self.samples.iter().enumerate() {
            let Some(value) = sample.values.get(metric).copied() else { continue };
            for frame in &sample.stack {
                let index = *frame as usize;
                if counted[index] != number {
                    counted[index] = number;
                    costs[index].cumulative += value;
                }
            }
            if let Some(leaf) = sample.stack.last() {
                costs[*leaf as usize].flat += value;
            }
        }
        costs.retain(|cost| cost.cumulative != 0);
        costs.sort_by_key(|cost| (std::cmp::Reverse(cost.flat), std::cmp::Reverse(cost.cumulative), cost.frame));
        costs
    }

    pub fn call_tree(&self, metric: usize, depth: usize, least: i64) -> CallNode {
        let mut nodes = vec![TreeNode::default()];
        for sample in &self.samples {
            let Some(value) = sample.values.get(metric).copied().filter(|value| *value != 0) else { continue };
            let mut at = 0;
            nodes[0].value += value;
            for frame in &sample.stack {
                let known = nodes[at].children.get(frame).copied();
                let next = known.unwrap_or(nodes.len());
                if known.is_none() {
                    nodes.push(TreeNode { frame: Some(*frame), ..TreeNode::default() });
                    nodes[at].children.insert(*frame, next);
                }
                nodes[next].value += value;
                at = next;
            }
        }
        pruned(&nodes, 0, depth, least)
    }
}

pub trait ProfileProvider: Send + Sync {
    fn command_lines(
        &self,
        configuration: &Configuration,
        context: &RunContext,
        kind: ProfileKind,
        report: &Path,
    ) -> Result<Vec<CommandLine>, String>;

    fn read(&self, configuration: &Configuration, context: &RunContext, report: &Path) -> Result<Profile, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(function: &str) -> ProfileFrame {
        ProfileFrame { function: function.to_owned(), file: None, line: 0 }
    }

    fn profile() -> Profile {
        Profile {
            metrics: vec![ProfileMetric { name: "cpu".to_owned(), unit: "nanoseconds".to_owned() }],
            frames: vec![frame("main"), frame("work"), frame("rest")],
            samples: vec![
                ProfileSample { stack: vec![0, 1, 1], values: vec![6] },
                ProfileSample { stack: vec![0, 2], values: vec![3] },
                ProfileSample { stack: vec![0], values: vec![1] },
            ],
            ..Profile::default()
        }
    }

    #[test]
    fn a_recursive_function_counts_once_per_sample_in_its_cumulative_cost() {
        let costs = profile().functions(0);
        assert_eq!(costs[0], FunctionCost { frame: 1, flat: 6, cumulative: 6 });
        assert_eq!(costs[2], FunctionCost { frame: 0, flat: 1, cumulative: 10 });
        assert_eq!(profile().total(0), 10);
    }

    #[test]
    fn the_call_tree_sums_shared_stacks_and_puts_the_costliest_call_first() {
        let tree = profile().call_tree(0, 8, 0);
        assert_eq!(tree.value, 10);
        let main = &tree.children[0];
        assert_eq!((main.frame, main.value), (Some(0), 10));
        assert_eq!(main.children.iter().map(|child| child.frame).collect::<Vec<_>>(), [Some(1), Some(2)]);
    }

    #[test]
    fn the_call_tree_says_how_many_calls_its_limits_left_out() {
        let tree = profile().call_tree(0, 8, 5);
        assert_eq!(tree.children[0].children.len(), 1);
        assert_eq!(tree.children[0].omitted, 1);
        let shallow = profile().call_tree(0, 1, 0);
        assert!(shallow.children[0].children.is_empty());
        assert_eq!(shallow.children[0].omitted, 2);
    }
}
