//! The Inspection Results tree: inspection → file → problem, under severity groups on request (IDEA's
//! default groups by inspection).

use super::inspect::{
    InspectionFileGroup, InspectionKindGroup, InspectionProblem, InspectionSeverityGroup, SeverityKey, sort_problems,
};
use crate::platform::collate;
use crate::platform_more::inspections::{InspectionRef, diagnostic_inspection, producer_of};

/// A row of the results tree; keys are unique across the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectionNode {
    Severity { key: String, severity: SeverityKey, count: usize, children: Vec<InspectionNode> },
    Inspection { key: String, name: String, severity: SeverityKey, count: usize, children: Vec<InspectionNode> },
    File { key: String, path: String, count: usize, children: Vec<InspectionNode> },
    Problem { key: String, problem: InspectionProblem },
}

impl InspectionNode {
    pub fn key(&self) -> &str {
        let (Self::Severity { key, .. }
        | Self::Inspection { key, .. }
        | Self::File { key, .. }
        | Self::Problem { key, .. }) = self;
        key
    }

    pub fn children(&self) -> &[InspectionNode] {
        match self {
            Self::Severity { children, .. } | Self::Inspection { children, .. } | Self::File { children, .. } => {
                children
            }
            Self::Problem { .. } => &[],
        }
    }

    /// The problems of the node and its descendants.
    pub fn problems(&self) -> Vec<&InspectionProblem> {
        match self {
            Self::Problem { problem, .. } => vec![problem],
            _ => self.children().iter().flat_map(InspectionNode::problems).collect(),
        }
    }
}

/// English source text of a severity group's row.
pub fn severity_label(severity: SeverityKey) -> &'static str {
    match severity {
        SeverityKey::Error => "Errors",
        SeverityKey::Warning => "Warnings",
        SeverityKey::Info => "Weak warnings and hints",
    }
}

/// Inspections across severities, merged by name (the same check may report errors and warnings) under
/// the most severe group they appear in; worst severity first, then by name.
fn merge_kinds(groups: &[InspectionSeverityGroup]) -> Vec<(InspectionKindGroup, SeverityKey)> {
    let mut merged: Vec<(InspectionKindGroup, SeverityKey)> = Vec::new();
    for group in groups {
        for kind in &group.kinds {
            let Some(i) = merged.iter().position(|(known, _)| known.kind == kind.kind) else {
                merged.push((kind.clone(), group.severity));
                continue;
            };
            let known = &mut merged[i].0;
            known.count += kind.count;
            for file in &kind.files {
                match known.files.iter_mut().find(|f| f.path == file.path) {
                    Some(same) => same.problems.extend(file.problems.iter().cloned()),
                    None => known.files.push(file.clone()),
                }
            }
        }
    }
    for (kind, _) in &mut merged {
        kind.files.sort_by(|a, b| collate::compare(&a.rel_path, &b.rel_path));
        for file in &mut kind.files {
            sort_problems(&mut file.problems);
        }
    }
    merged.sort_by(|(a, sa), (b, sb)| sa.cmp(sb).then_with(|| collate::compare(&a.kind, &b.kind)));
    merged
}

fn file_node(file: &InspectionFileGroup, kind_key: &str) -> InspectionNode {
    let key = format!("{kind_key}\u{0}{}", file.path);
    InspectionNode::File {
        children: file
            .problems
            .iter()
            .enumerate()
            .map(|(i, problem)| InspectionNode::Problem { key: format!("{key}\u{0}{i}"), problem: problem.clone() })
            .collect(),
        key,
        path: file.path.clone(),
        count: file.problems.len(),
    }
}

fn kind_node(kind: &InspectionKindGroup, severity: SeverityKey, prefix: &str) -> InspectionNode {
    let key = format!("{prefix}\u{0}{}", kind.kind);
    InspectionNode::Inspection {
        children: kind.files.iter().map(|file| file_node(file, &key)).collect(),
        key,
        name: kind.kind.clone(),
        severity,
        count: kind.count,
    }
}

/// The results tree: inspection → file → problem, under severity groups with `by_severity`.
pub fn build_inspection_nodes(groups: &[InspectionSeverityGroup], by_severity: bool) -> Vec<InspectionNode> {
    if !by_severity {
        return merge_kinds(groups).iter().map(|(kind, severity)| kind_node(kind, *severity, "kind")).collect();
    }
    groups
        .iter()
        .map(|group| InspectionNode::Severity {
            key: group.severity.as_str().to_owned(),
            severity: group.severity,
            count: group.count,
            children: group.kinds.iter().map(|kind| kind_node(kind, group.severity, group.severity.as_str())).collect(),
        })
        .collect()
}

/// The one inspection all of the node's problems come from (for its context menu's inspection controls).
pub fn node_inspection(node: &InspectionNode) -> Option<InspectionRef> {
    let mut found: Option<InspectionRef> = None;
    for problem in node.problems() {
        let inspection = diagnostic_inspection(producer_of(&problem.d, None), problem.d.code.as_deref())?;
        if found.as_ref().is_some_and(|known| known.id != inspection.id) {
            return None;
        }
        found = Some(inspection);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::super::inspect::InspectedFileResult;
    use super::super::inspect::group_inspection;
    use super::super::inspect::tests::{diag, files};
    use super::*;
    use ide_core::Severity;

    fn keys<'a>(nodes: &'a [InspectionNode], out: &mut Vec<&'a str>) {
        for node in nodes {
            out.push(node.key());
            keys(node.children(), out);
        }
    }

    #[test]
    fn builds_a_tree_with_unique_keys() {
        let nodes = build_inspection_nodes(&group_inspection(&files()), true);
        let mut all = Vec::new();
        keys(&nodes, &mut all);
        let unique: std::collections::HashSet<&str> = all.iter().copied().collect();
        assert_eq!(unique.len(), all.len());
        assert!(matches!(nodes[0], InspectionNode::Severity { .. }));
    }

    #[test]
    fn groups_by_inspection_by_default_worst_severity_first_merging_severities() {
        let mut with_warning = files();
        with_warning.push(InspectedFileResult {
            path: "/p/d.ts".to_owned(),
            rel_path: "d.ts".to_owned(),
            diagnostics: vec![diag(5, Severity::Warning, Some("ts"), Some("2304"))],
        });
        let nodes = build_inspection_nodes(&group_inspection(&with_warning), false);
        let shape: Vec<(&str, SeverityKey, usize)> = nodes
            .iter()
            .filter_map(|node| match node {
                InspectionNode::Inspection { name, severity, count, .. } => Some((name.as_str(), *severity, *count)),
                _ => None,
            })
            .collect();
        assert_eq!(
            shape,
            [
                ("ts(2304)", SeverityKey::Error, 4),
                ("ts(7006)", SeverityKey::Error, 1),
                ("eslint(no-unused-vars)", SeverityKey::Warning, 1),
                ("Language server", SeverityKey::Info, 1),
            ]
        );
        let paths: Vec<&str> = nodes[0]
            .children()
            .iter()
            .filter_map(|node| match node {
                InspectionNode::File { path, .. } => Some(path.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(paths, ["/p/b.ts", "/p/c.ts", "/p/d.ts"]);
    }
}
