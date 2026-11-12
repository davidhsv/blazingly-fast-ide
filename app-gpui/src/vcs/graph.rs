//! Commit graph lanes for the Git log (computed from parents, for commits in topological order,
//! children before parents), plus the graph transformations of the log's Graph / View options.

use crate::platform::collate;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    Normal,
    Filtered,
    Collapsed,
    Shallow,
    Unloaded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Arrow {
    Up,
    Down,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GraphSegment {
    /// The edge the segment is part of: the child's id, a NUL and the parent's id. Set, with `kind` and
    /// `source`, when the commits carry references.
    pub edge: Option<String>,
    pub kind: Option<EdgeKind>,
    /// Id of the edge's child commit.
    pub source: Option<String>,
    /// Lane at the segment's start (top edge for `up`, node level for `down`).
    pub from: usize,
    /// Lane at the segment's end (node level for `up`, bottom edge for `down`).
    pub to: usize,
    pub color: u32,
    /// Stands for collapsed commits (Collapse Linear Branches).
    pub dashed: bool,
    /// A long edge continues out of sight in this direction (Show Long Edges off).
    pub arrow: Option<Arrow>,
    /// Destination row of a long-edge arrow in the displayed graph; -1 below the loaded rows.
    pub target_row: Option<isize>,
    /// Drawn lane positions when they differ from `from` / `to` (see [`straighten_lane_changes`]).
    pub from_x: Option<f64>,
    pub to_x: Option<f64>,
}

impl GraphSegment {
    fn new(from: usize, to: usize, color: u32, dashed: bool) -> Self {
        Self { from, to, color, dashed, ..Self::default() }
    }

    /// The same segment between other lanes.
    fn between(&self, from: usize, to: usize) -> Self {
        Self { from, to, ..self.clone() }
    }

    /// The segment as part of the edge from `source` to its parent `target`, when edges are `tracked`.
    fn of_edge(mut self, tracked: bool, source: &str, target: &str, kind: EdgeKind) -> Self {
        if tracked {
            self.edge = Some(format!("{source}\0{target}"));
            self.source = Some(source.to_owned());
            self.kind = Some(kind);
        }
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GraphRow {
    /// The commit's id, when the commits carry references.
    pub id: Option<String>,
    /// Lane of the commit's node.
    pub lane: usize,
    pub color: u32,
    /// Top half of the row: lines entering from the previous row.
    pub up: Vec<GraphSegment>,
    /// Bottom half: lines leaving towards the next row.
    pub down: Vec<GraphSegment>,
    /// Number of lane slots this row spans.
    pub width: usize,
    /// Graph ids corresponding to each half-edge, for fragment actions; empty when not known.
    pub up_targets: Vec<String>,
    pub down_targets: Vec<String>,
}

/// A line of the graph on its way down to the commit `id`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lane {
    pub id: String,
    /// Id of the child commit the line comes from.
    pub source: String,
    pub kind: EdgeKind,
    pub color: u32,
    pub dashed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaneSlot {
    /// A position left of a lane that was placed by its branch's layout, which no line has used yet.
    /// It is given only to a commit whose layout asks for it: looking for a free slot skips it, and it
    /// keeps the slots before it from being trimmed.
    Gap,
    /// A position whose line has ended; the next line that needs a slot takes the first of these.
    Free,
    Taken(Lane),
}

impl LaneSlot {
    fn lane(&self) -> Option<&Lane> {
        match self {
            Self::Taken(lane) => Some(lane),
            Self::Gap | Self::Free => None,
        }
    }
}

/// The lanes open below the last laid out row, so that pages can be appended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphState {
    pub lanes: Vec<LaneSlot>,
    pub next_color: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphCommit {
    pub id: String,
    pub parents: Vec<String>,
    /// Per parent: the link skips collapsed commits. A parent without an entry is linked directly.
    pub dashed: Vec<bool>,
    /// The commit's references. `None` when the log does not carry them: the rows then hold no ids
    /// and no edge identities.
    pub refs: Option<Vec<String>>,
    /// Those of `refs` that are remote branches.
    pub remote_names: Vec<String>,
    /// Per parent: the kind of the link. A parent without an entry is `Filtered` when dashed, else
    /// `Normal`.
    pub edge_types: Vec<EdgeKind>,
}

impl GraphCommit {
    fn refs(&self) -> &[String] {
        self.refs.as_deref().unwrap_or_default()
    }

    fn is_dashed(&self, parent: usize) -> bool {
        self.dashed.get(parent).copied().unwrap_or(false)
    }

    fn edge_kind(&self, parent: usize, dashed: bool) -> EdgeKind {
        let implied = if dashed { EdgeKind::Filtered } else { EdgeKind::Normal };
        self.edge_types.get(parent).copied().unwrap_or(implied)
    }
}

/// Row of each commit by id (the last one for an id listed twice).
fn positions_of(commits: &[GraphCommit]) -> HashMap<&str, usize> {
    commits.iter().enumerate().map(|(index, commit)| (commit.id.as_str(), index)).collect()
}

fn first_free(lanes: &[LaneSlot]) -> usize {
    lanes.iter().position(|slot| *slot == LaneSlot::Free).unwrap_or(lanes.len())
}

/// Colour index of the lane owned by `owner`: the same whatever else the log shows.
fn owner_color(owner: &str) -> u32 {
    owner.chars().fold(2_166_136_261u32, |hash, c| {
        let mut units = [0u16; 2];
        (hash ^ u32::from(c.encode_utf16(&mut units)[0])).wrapping_mul(16_777_619)
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComputedGraph {
    pub rows: Vec<GraphRow>,
    pub state: GraphState,
}

/// Lays out `commits` (continuing from `state`, so pages can be appended). Each lane keeps its
/// position while it lives; freed slots are reused.
pub fn compute_graph(commits: &[GraphCommit], state: GraphState) -> ComputedGraph {
    let GraphState { mut lanes, mut next_color } = state;
    let track = commits.iter().any(|c| c.refs.is_some());
    let positions = positions_of(commits);
    let layout = graph_layout(commits);
    let ref_aware = commits.iter().any(|c| !c.refs().is_empty());
    let stable_color = |id: &str, fallback: u32| {
        if !ref_aware {
            return fallback;
        }
        owner_color(positions.get(id).map_or(id, |&index| layout.owners[index].as_str()))
    };

    let mut rows: Vec<GraphRow> = Vec::with_capacity(commits.len());
    for (index, c) in commits.iter().enumerate() {
        let matching: Vec<usize> = lanes
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.lane().is_some_and(|l| l.id == c.id))
            .map(|(i, _)| i)
            .collect();
        let continued = matching.first().and_then(|&first| lanes[first].lane().map(|l| (first, l.color)));
        let (lane, color) = continued.unwrap_or_else(|| {
            let position = positions.get(c.id.as_str()).copied().unwrap_or(index);
            let preferred = if ref_aware { layout.layout[position].saturating_sub(1) } else { first_free(&lanes) };
            let taken = matches!(lanes.get(preferred), Some(LaneSlot::Taken(_)));
            let color = stable_color(&c.id, next_color);
            next_color += 1;
            (if taken { first_free(&lanes) } else { preferred }, color)
        });
        let width_before = lanes.len();

        let mut up = Vec::new();
        let mut up_targets = Vec::new();
        for (i, l) in lanes.iter().enumerate().filter_map(|(i, slot)| Some((i, slot.lane()?))) {
            let to = if l.id == c.id { lane } else { i };
            up.push(GraphSegment::new(i, to, l.color, l.dashed).of_edge(track, &l.source, &l.id, l.kind));
            up_targets.push(l.id.clone());
        }
        for &m in &matching {
            lanes[m] = LaneSlot::Free;
        }
        if lane >= lanes.len() {
            lanes.resize(lane + 1, LaneSlot::Gap);
        }
        lanes[lane] = LaneSlot::Free;

        let mut down = Vec::new();
        let mut down_targets = Vec::new();
        for (i, l) in lanes.iter().enumerate().filter_map(|(i, slot)| Some((i, slot.lane()?))) {
            down.push(GraphSegment::new(i, i, l.color, l.dashed).of_edge(track, &l.source, &l.id, l.kind));
            down_targets.push(l.id.clone());
        }

        for (k, p) in c.parents.iter().enumerate() {
            let dashed = c.is_dashed(k);
            let kind = c.edge_kind(k, dashed);
            let loaded = positions.contains_key(p.as_str());
            let lane_kind = if !loaded && kind != EdgeKind::Shallow { EdgeKind::Unloaded } else { kind };
            let edge = |segment: GraphSegment| segment.of_edge(track, &c.id, p, lane_kind);
            let line = |color: u32| Lane { id: p.clone(), source: c.id.clone(), kind: lane_kind, color, dashed };
            down_targets.push(p.clone());
            if track && kind == EdgeKind::Shallow {
                let mut stub = edge(GraphSegment::new(lane, lane, color, dashed));
                stub.arrow = Some(Arrow::Down);
                stub.target_row = Some(-1);
                down.push(stub);
                continue;
            }
            // The first parent continues this lane even if another lane also leads to it
            // (both lines then converge at the parent's node, like `git log --graph`).
            if k == 0 {
                lanes[lane] = LaneSlot::Taken(line(color));
                down.push(edge(GraphSegment::new(lane, lane, color, dashed)));
                continue;
            }
            let existing = lanes.iter().enumerate().find_map(|(i, slot)| {
                let l = slot.lane()?;
                (l.id == *p).then_some((i, l.color))
            });
            if let Some((existing, existing_color)) = existing {
                down.push(edge(GraphSegment::new(lane, existing, existing_color, dashed)));
                continue;
            }
            let slot = first_free(&lanes);
            let new_color = stable_color(p, next_color);
            next_color += 1;
            if slot == lanes.len() {
                lanes.push(LaneSlot::Taken(line(new_color)));
            } else {
                lanes[slot] = LaneSlot::Taken(line(new_color));
            }
            down.push(edge(GraphSegment::new(lane, slot, new_color, dashed)));
        }

        while lanes.last() == Some(&LaneSlot::Free) {
            lanes.pop();
        }
        let width_after = down.iter().map(|segment| segment.to + 1).max().unwrap_or(0);
        rows.push(GraphRow {
            id: track.then(|| c.id.clone()),
            lane,
            color,
            up,
            down,
            width: width_before.max(width_after).max(lane + 1),
            up_targets,
            down_targets,
        });
    }
    if track && let Some(last) = rows.last_mut() {
        for segment in last.down.iter_mut().filter(|segment| segment.kind == Some(EdgeKind::Unloaded)) {
            segment.arrow = Some(Arrow::Down);
            segment.target_row = Some(-1);
        }
    }
    ComputedGraph { rows, state: GraphState { lanes, next_color } }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollapsedGraph {
    /// Indices, into the commits given, of those that stay visible.
    pub visible: Vec<usize>,
    /// The visible commits linked to each other, in the same order as `visible`.
    pub graph: Vec<GraphCommit>,
}

/// Collapse Linear Branches: hides the commits of linear chains (one parent, one child, no
/// references) so that only branch points, merges, tips and referenced commits stay, linked by dashed
/// edges. Commits in `keep` (HEAD) stay visible. With `only`, no other commit than these is hidden.
pub fn collapse_linear(
    commits: &[GraphCommit],
    keep: &HashSet<String>,
    only: Option<&HashSet<String>>,
) -> CollapsedGraph {
    let by_id: HashMap<&str, &GraphCommit> = commits.iter().map(|c| (c.id.as_str(), c)).collect();
    let mut children: HashMap<&str, usize> = HashMap::new();
    for parent in commits.iter().flat_map(|c| &c.parents) {
        *children.entry(parent.as_str()).or_default() += 1;
    }
    let hidden = |c: &GraphCommit| {
        c.parents.len() == 1
            && children.get(c.id.as_str()) == Some(&1)
            && c.refs().is_empty()
            && !keep.contains(&c.id)
            && only.is_none_or(|only| only.contains(&c.id))
    };

    let mut out = CollapsedGraph { visible: Vec::new(), graph: Vec::new() };
    for (index, c) in commits.iter().enumerate() {
        if hidden(c) {
            continue;
        }
        let mut parents: Vec<String> = Vec::new();
        let mut dashed: Vec<bool> = Vec::new();
        let mut edge_types: Vec<EdgeKind> = Vec::new();
        for (i, p) in c.parents.iter().enumerate() {
            let mut cur = p.as_str();
            let mut skipped = c.is_dashed(i);
            let mut kind = c.edge_kind(i, skipped);
            for _ in 0..commits.len() {
                let Some(&next) = by_id.get(cur) else { break };
                if !hidden(next) {
                    break;
                }
                cur = next.parents[0].as_str();
                skipped = true;
                kind = EdgeKind::Collapsed;
            }
            // Two collapsed lines can lead to the same commit: draw one edge.
            match parents.iter().position(|seen| seen == cur) {
                Some(seen) => dashed[seen] |= skipped,
                None => {
                    parents.push(cur.to_owned());
                    dashed.push(skipped);
                    edge_types.push(kind);
                }
            }
        }
        out.visible.push(index);
        out.graph.push(GraphCommit {
            id: c.id.clone(),
            parents,
            dashed,
            refs: c.refs.clone(),
            remote_names: c.remote_names.clone(),
            edge_types,
        });
    }
    out
}

/// Ids reachable from the given heads (log order: children before parents).
pub fn reachable_from<'a>(commits: &'a [GraphCommit], heads: impl IntoIterator<Item = &'a str>) -> HashSet<&'a str> {
    let mut out: HashSet<&str> = heads.into_iter().collect();
    for c in commits {
        if out.contains(c.id.as_str()) {
            out.extend(c.parents.iter().map(String::as_str));
        }
    }
    out
}

/// Where each commit sits in the ref-priority layout of the log's branches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphLayout {
    /// Per commit: the 1-based number of its line in the layout.
    pub layout: Vec<usize>,
    /// Per commit: the reference (or head commit) its line was laid out from, a NUL and the line's
    /// number counted from that head's first line.
    pub owners: Vec<String>,
    /// Per commit: the rows of its loaded parents.
    pub parents: Vec<Vec<usize>>,
    /// Per commit: the rows of its children.
    pub children: Vec<Vec<usize>>,
}

/// Sort key of a reference of `commit`: HEAD, then the main branch, local branches, remote branches,
/// tags.
fn ref_rank(commit: &GraphCommit, name: &str) -> String {
    let rank = if name == "HEAD" {
        0
    } else if name == "main" || name == "master" {
        1
    } else if name.starts_with("tag: ") {
        4
    } else if commit.remote_names.iter().any(|remote| remote == name) {
        3
    } else {
        2
    };
    format!("{rank}{name}")
}

/// Ref-priority DFS layout, shared by Bek fragment recognition and lane colours.
/// Adapted from IntelliJ GraphLayoutBuilder (JetBrains contributors, Apache-2.0).
pub fn graph_layout(commits: &[GraphCommit]) -> GraphLayout {
    let positions = positions_of(commits);
    let loaded = |c: &GraphCommit| -> Vec<usize> {
        c.parents.iter().filter_map(|id| positions.get(id.as_str()).copied()).collect()
    };
    let parents: Vec<Vec<usize>> = commits.iter().map(loaded).collect();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); commits.len()];
    for (i, of_commit) in parents.iter().enumerate() {
        for &p in of_commit {
            children[p].push(i);
        }
    }
    let priority = |c: &GraphCommit| {
        let ranks = c.refs().iter().map(|name| ref_rank(c, name));
        ranks.min_by(|a, b| a.encode_utf16().cmp(b.encode_utf16())).unwrap_or_else(|| format!("5{}", c.id))
    };
    let mut heads: Vec<(String, usize)> = commits
        .iter()
        .enumerate()
        .filter(|(i, c)| children[*i].is_empty() || !c.refs().is_empty())
        .map(|(i, c)| (priority(c), i))
        .collect();
    heads.sort_by(|a, b| collate::compare(&a.0, &b.0));

    let mut layout = vec![0usize; commits.len()];
    let mut owners = vec![String::new(); commits.len()];
    let mut next = 1usize;
    for (_, head) in heads {
        if layout[head] != 0 {
            continue;
        }
        let commit = &commits[head];
        let branches = commit.refs().iter().map(String::as_str).filter(|name| *name != "HEAD");
        let owner = branches
            .min_by(|a, b| collate::compare(&ref_rank(commit, a), &ref_rank(commit, b)))
            .unwrap_or(if commit.refs().iter().any(|name| name == "HEAD") { "HEAD" } else { commit.id.as_str() });
        let start = next;
        let mut stack = vec![head];
        while let Some(&node) = stack.last() {
            let first = layout[node] == 0;
            if first {
                layout[node] = next;
                owners[node] = format!("{owner}\0{}", next - start);
            }
            match parents[node].iter().copied().find(|&p| layout[p] == 0) {
                Some(parent) => stack.push(parent),
                None => {
                    if first {
                        next += 1;
                    }
                    stack.pop();
                }
            }
        }
    }
    GraphLayout { layout, owners, parents, children }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinearizedGraph {
    /// The commits given, linked as the linearized log draws them.
    pub graph: Vec<GraphCommit>,
    /// Per collapsed edge (the child's id, a NUL and the parent's id): the merge whose incoming
    /// commits it puts in a line.
    pub fragments: BTreeMap<String, String>,
    /// Per merge with such a fragment: the commits of the fragment.
    pub nodes: BTreeMap<String, BTreeSet<String>>,
}

/// A link of the graph walked while looking for a merge's fragment.
struct Link<'g> {
    up: usize,
    down: Option<usize>,
    id: &'g str,
}

/// The commits a merge brings in, when they can be drawn in a line above its other parent.
struct Fragment {
    body: HashSet<usize>,
    /// Commits whose links leave the fragment, in the order they were met.
    tails: Vec<usize>,
    /// Per tail: the parents below the merge's other parent that the fragment's line replaces.
    tail_edges: BTreeMap<usize, HashSet<String>>,
}

impl Fragment {
    fn add_tail(&mut self, up: usize) {
        if !self.tails.contains(&up) {
            self.tails.push(up);
        }
    }
}

/// The links from the commit at row `up` to its parents.
fn links<'g>(
    graph: &'g [GraphCommit],
    positions: &'g HashMap<&str, usize>,
    up: usize,
) -> impl Iterator<Item = Link<'g>> {
    graph[up].parents.iter().map(move |id| Link { up, down: positions.get(id.as_str()).copied(), id: id.as_str() })
}

/// The fragment of the merge whose loaded parents are at rows `right` and `left` (`right` first).
fn find_fragment(
    graph: &[GraphCommit],
    positions: &HashMap<&str, usize>,
    layout: &[usize],
    right: usize,
    left: usize,
) -> Option<Fragment> {
    // Commits near below `left` that it leads to: a link may end there without breaking the line.
    let mut magic: HashSet<usize> = HashSet::new();
    let mut pending: Vec<&str> = graph[left].parents.iter().map(String::as_str).collect();
    while let Some(id) = pending.pop() {
        let Some(&i) = positions.get(id) else { continue };
        if i > left + LONG_EDGE || !magic.insert(i) {
            continue;
        }
        pending.extend(graph[i].parents.iter().map(String::as_str));
    }

    let mut fragment = Fragment { body: HashSet::new(), tails: Vec::new(), tail_edges: BTreeMap::new() };
    let (mut rows, mut size, mut old) = (1usize, 1usize, false);
    let mut visited: HashSet<(usize, &str)> = HashSet::new();
    let mut queue: Vec<Link> = links(graph, positions, right).collect();
    loop {
        // Nearest row last; links to commits that are not loaded first.
        queue.sort_by_key(|link| Reverse(link.down.unwrap_or(usize::MAX)));
        let Some(link) = queue.pop() else { break };
        if !visited.insert((link.up, link.id)) {
            continue;
        }
        let in_body = fragment.body.contains(&link.up);
        let Some(next) = link.down else {
            if !in_body {
                fragment.add_tail(link.up);
            }
            continue;
        };
        if next == left {
            if !in_body {
                fragment.add_tail(link.up);
            }
            old = true;
        } else if (right + rows..left).contains(&next) {
            rows = next - right + 1;
            size += 1;
            queue.extend(links(graph, positions, next));
            fragment.body.insert(link.up);
        } else if next > left {
            let (li, ll, rl) = (layout[next], layout[left], layout[right]);
            let allowed = if ll > rl && !old {
                next <= left + LONG_EDGE && magic.contains(&next)
            } else if (li > ll && li < rl) || li == ll {
                true
            } else if li >= rl {
                false
            } else if next > left + LONG_EDGE {
                fragment.tail_edges.contains_key(&link.up) || in_body
            } else {
                magic.contains(&next)
            };
            if !allowed {
                return None;
            }
            if !in_body {
                fragment.add_tail(link.up);
                fragment.tail_edges.entry(link.up).or_default().insert(link.id.to_owned());
            }
        }
        if size >= 200 {
            return None;
        }
    }
    (!fragment.tails.is_empty()).then_some(fragment)
}

/// Keeps the parents of `commit` that `keep` accepts, with their dashes.
fn retain_parents(commit: &mut GraphCommit, keep: impl Fn(&str) -> bool) {
    let mut kept = commit.parents.iter().map(|parent| keep(parent));
    commit.dashed.retain(|_| kept.next().unwrap_or(true));
    commit.parents.retain(|parent| keep(parent));
}

fn linearize(
    commits: &[GraphCommit],
    positions: &HashMap<&str, usize>,
    layout: &[usize],
    expand: &HashSet<String>,
) -> LinearizedGraph {
    let copy = |c: &GraphCommit| GraphCommit {
        id: c.id.clone(),
        parents: c.parents.clone(),
        dashed: (0..c.parents.len()).map(|parent| c.is_dashed(parent)).collect(),
        refs: c.refs.clone(),
        remote_names: c.remote_names.clone(),
        edge_types: c.edge_types.clone(),
    };
    let mut graph: Vec<GraphCommit> = commits.iter().map(copy).collect();
    let mut fragments: BTreeMap<String, String> = BTreeMap::new();
    let mut nodes: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for merge in (0..graph.len()).rev() {
        let mut down: Vec<usize> =
            graph[merge].parents.iter().filter_map(|p| positions.get(p.as_str()).copied()).collect();
        down.sort_unstable();
        if down.len() != 2 || graph[merge].parents.len() != 2 {
            continue;
        }
        let (right, left) = (down[0], down[1]);
        let Some(fragment) = find_fragment(&graph, positions, layout, right, left) else { continue };
        let id = graph[merge].id.clone();
        let members = [merge, left, right].into_iter().chain(fragment.body).chain(fragment.tails.iter().copied());
        nodes.insert(id.clone(), members.map(|i| graph[i].id.clone()).collect());
        if expand.contains(&id) {
            continue;
        }
        for (&up, removed) in &fragment.tail_edges {
            retain_parents(&mut graph[up], |parent| !removed.contains(parent));
        }
        let target = graph[left].id.clone();
        for &tail in &fragment.tails {
            let c = &mut graph[tail];
            match c.parents.iter().position(|parent| *parent == target) {
                Some(at) => c.dashed[at] = true,
                None => {
                    c.parents.push(target.clone());
                    c.dashed.push(true);
                }
            }
            fragments.insert(format!("{}\0{target}", c.id), id.clone());
        }
        retain_parents(&mut graph[merge], |parent| parent != target);
    }
    for (index, c) in graph.iter_mut().enumerate() {
        let original = &commits[positions.get(c.id.as_str()).copied().unwrap_or(index)];
        let kind = |(i, parent): (usize, &String)| {
            if fragments.contains_key(&format!("{}\0{parent}", c.id)) {
                return EdgeKind::Collapsed;
            }
            let given = original.parents.iter().position(|p| p == parent).and_then(|at| original.edge_types.get(at));
            given.copied().unwrap_or(if c.dashed[i] { EdgeKind::Filtered } else { EdgeKind::Normal })
        };
        c.edge_types = c.parents.iter().enumerate().map(kind).collect();
    }
    LinearizedGraph { graph, fragments, nodes }
}

/// LinearBek fragment recognition and nested expansion: the commits a merge brings in are drawn in a
/// line above its other parent, unless the merge is in `expanded`.
/// Adapted from JetBrains LinearBekGraphBuilder, Copyright 2000-2025, Apache-2.0.
pub fn linearize_merges(commits: &[GraphCommit], expanded: &HashSet<String>) -> LinearizedGraph {
    let positions = positions_of(commits);
    let layout = graph_layout(commits).layout;
    let mut expand = expanded.clone();
    loop {
        let result = linearize(commits, &positions, &layout, &expand);
        // Expanding a containing merge restores its nested fragments as well.
        let nested: Vec<String> = expand
            .iter()
            .filter_map(|id| result.nodes.get(id))
            .flatten()
            .filter(|node| result.nodes.contains_key(*node) && !expand.contains(*node))
            .cloned()
            .collect();
        if nested.is_empty() {
            return result;
        }
        expand.extend(nested);
    }
}

/// Rows a straight lane has to pass through for its middle to be hidden when long edges are not shown.
pub const LONG_EDGE: usize = 30;
/// Rows of a hidden long edge still drawn next to each of its ends.
const LONG_EDGE_STUB: usize = 2;

fn on_lane(segment: &GraphSegment, lane: usize) -> bool {
    segment.from == lane && segment.to == lane
}

/// Lanes a row's vertical lines pass straight through (other than its node's).
fn passing_lanes(row: &GraphRow) -> BTreeSet<usize> {
    let leaves_on = |lane: usize| row.down.iter().any(|d| on_lane(d, lane));
    let passes = |s: &&GraphSegment| s.from == s.to && s.from != row.lane && leaves_on(s.from);
    row.up.iter().filter(passes).map(|s| s.from).collect()
}

/// Removes the segments of `lane` and the targets listed for them.
fn drop_lane(segments: &mut Vec<GraphSegment>, targets: &mut Vec<String>, lane: usize) {
    let mut kept = segments.iter().map(|segment| !on_lane(segment, lane));
    targets.retain(|_| kept.next().unwrap_or(true));
    segments.retain(|segment| !on_lane(segment, lane));
}

/// Hides the middle of the run of `lane` through the rows `start..end`, leaving arrows on its stubs.
fn hide_run(rows: &mut [GraphRow], lane: usize, start: usize, end: usize) {
    let from = start + LONG_EDGE_STUB;
    let until = end.saturating_sub(LONG_EDGE_STUB);
    for row in rows.iter_mut().take(until).skip(from) {
        drop_lane(&mut row.up, &mut row.up_targets, lane);
        drop_lane(&mut row.down, &mut row.down_targets, lane);
    }
    if let Some(row) = rows.get_mut(from - 1) {
        for segment in row.down.iter_mut().filter(|segment| on_lane(segment, lane)) {
            segment.arrow = Some(Arrow::Down);
            segment.target_row = Some(end as isize);
        }
    }
    if let Some(row) = rows.get_mut(until) {
        for segment in row.up.iter_mut().filter(|segment| on_lane(segment, lane)) {
            segment.arrow = Some(Arrow::Up);
            segment.target_row = Some(start as isize - 1);
        }
    }
}

/// Show Long Edges off: the middle of a straight run of a lane through `min_length` (normally
/// [`LONG_EDGE`]) or more rows is not drawn; the visible stubs end in arrows (IDEA's long-edge arrows).
pub fn hide_long_edges(rows: &[GraphRow], min_length: usize) -> Vec<GraphRow> {
    let mut out = rows.to_vec();
    // One pass over the rows, following every lane's current run at once.
    let mut runs: BTreeMap<usize, usize> = BTreeMap::new();
    for r in 0..=rows.len() {
        let through = rows.get(r).map(passing_lanes).unwrap_or_default();
        let ended: Vec<(usize, usize)> =
            runs.iter().filter(|(lane, _)| !through.contains(*lane)).map(|(&lane, &start)| (lane, start)).collect();
        for (lane, start) in ended {
            if r - start >= min_length {
                hide_run(&mut out, lane, start, r);
            }
            runs.remove(&lane);
        }
        for &lane in &through {
            runs.entry(lane).or_insert(r);
        }
    }
    out
}

/// The distinct lanes, in order: a lane's rank is its index.
fn ranks(lanes: impl Iterator<Item = usize>) -> Vec<usize> {
    let mut out: Vec<usize> = lanes.collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn rank_of(ranks: &[usize], lane: usize) -> usize {
    ranks.binary_search(&lane).unwrap_or_else(|at| at)
}

/// Packs each row's lanes side by side, in their order: lanes laid out by branch leave gaps (one per
/// branch) and hidden long edges free theirs. Each row boundary and each node level is packed on its
/// own; adjacent rows share the boundary, so lines stay joined.
pub fn compact_lanes(rows: &[GraphRow]) -> Vec<GraphRow> {
    let boundary = |r: usize| {
        let below = rows.get(r).into_iter().flat_map(|row| row.up.iter().map(|s| s.from));
        let above = r.checked_sub(1).and_then(|at| rows.get(at));
        let above = above.into_iter().flat_map(|row| row.down.iter().map(|s| s.to));
        ranks(below.chain(above))
    };
    let boundaries: Vec<Vec<usize>> = (0..=rows.len()).map(boundary).collect();
    let compact = |(r, row): (usize, &GraphRow)| {
        let at_node = row.up.iter().map(|s| s.to).chain(row.down.iter().map(|s| s.from));
        let node = ranks(std::iter::once(row.lane).chain(at_node));
        let (top, bottom) = (&boundaries[r], &boundaries[r + 1]);
        GraphRow {
            id: row.id.clone(),
            lane: rank_of(&node, row.lane),
            color: row.color,
            up: row.up.iter().map(|s| s.between(rank_of(top, s.from), rank_of(&node, s.to))).collect(),
            down: row.down.iter().map(|s| s.between(rank_of(&node, s.from), rank_of(bottom, s.to))).collect(),
            width: node.len().max(top.len()).max(bottom.len()),
            up_targets: row.up_targets.clone(),
            down_targets: row.down_targets.clone(),
        }
    };
    rows.iter().enumerate().map(compact).collect()
}

fn vertical(segment: &GraphSegment, lane: usize) -> bool {
    on_lane(segment, lane) && segment.arrow.is_none()
}

/// Whether more than one of the segments ends at `lane`.
fn shared_end(segments: &[GraphSegment], lane: usize) -> bool {
    segments.iter().filter(|segment| segment.to == lane).count() > 1
}

fn halfway(segment: &GraphSegment) -> f64 {
    (segment.from + segment.to) as f64 / 2.0
}

/// Draws a lane change as one straight diagonal across the row boundary, like IDEA: a half-row segment
/// that changes lane and the vertical half of the neighbouring row that continues it meet halfway
/// between the two lanes. Only the drawn positions change; `from` / `to` stay the lanes.
pub fn straighten_lane_changes(rows: &[GraphRow]) -> Vec<GraphRow> {
    let mut out = rows.to_vec();
    for r in 0..out.len() {
        // Leaving the node towards another lane: continues straight down in the next row.
        if let ([.., row], [next, ..]) = out.split_at_mut(r + 1) {
            for i in 0..row.down.len() {
                let s = &row.down[i];
                // Another line ending at the same lane (a merge into a lane that continues) keeps its position.
                if s.from == s.to || s.arrow.is_some() || shared_end(&row.down, s.to) {
                    continue;
                }
                let continues = |u: &GraphSegment| vertical(u, s.to) && u.edge == s.edge && u.from_x.is_none();
                let Some(j) = next.up.iter().position(continues) else { continue };
                let mid = halfway(s);
                row.down[i].to_x = Some(mid);
                next.up[j].from_x = Some(mid);
            }
        }
        // Converging into the node from another lane: continues a vertical line of the previous row.
        if let ([.., prev], [row, ..]) = out.split_at_mut(r) {
            for i in 0..row.up.len() {
                let s = &row.up[i];
                if s.from == s.to || s.arrow.is_some() || shared_end(&prev.down, s.from) {
                    continue;
                }
                let continued = |d: &GraphSegment| vertical(d, s.from) && d.edge == s.edge && d.to_x.is_none();
                let Some(j) = prev.down.iter().position(continued) else { continue };
                let mid = halfway(s);
                row.up[i].from_x = Some(mid);
                prev.down[j].to_x = Some(mid);
            }
        }
    }
    out
}

/// Lane colours (theme tokens), cycled by colour index.
pub const LANE_COLORS: [&str; 8] = [
    "var(--accent)",
    "var(--vcs-added)",
    "var(--vcs-modified)",
    "var(--syn-annotation)",
    "var(--syn-number)",
    "var(--syn-string)",
    "var(--syn-type)",
    "var(--danger)",
];

pub fn lane_color(index: u32) -> &'static str {
    LANE_COLORS[index as usize % LANE_COLORS.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    /// A commit of a log without references.
    fn c(id: &str, parents: &[&str]) -> GraphCommit {
        GraphCommit { id: id.to_owned(), parents: strings(parents), ..GraphCommit::default() }
    }

    /// A commit of a log with references.
    fn commit(id: &str, parents: &[&str], refs: &[&str]) -> GraphCommit {
        GraphCommit { refs: Some(strings(refs)), ..c(id, parents) }
    }

    fn graph(commits: &[GraphCommit]) -> Vec<GraphRow> {
        compute_graph(commits, GraphState::default()).rows
    }

    fn seg(from: usize, to: usize, color: u32) -> GraphSegment {
        GraphSegment::new(from, to, color, false)
    }

    fn ids(commits: &[GraphCommit]) -> Vec<&str> {
        commits.iter().map(|commit| commit.id.as_str()).collect()
    }

    fn parents(commits: &[GraphCommit]) -> Vec<&[String]> {
        commits.iter().map(|commit| commit.parents.as_slice()).collect()
    }

    fn set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    // src/features/vcs/__tests__/graph.test.ts › computeGraph › keeps a linear history in lane 0
    #[test]
    fn keeps_a_linear_history_in_lane_0() {
        let rows = graph(&[c("c", &["b"]), c("b", &["a"]), c("a", &[])]);
        assert_eq!(rows.iter().map(|r| r.lane).collect::<Vec<_>>(), [0, 0, 0]);
        assert!(rows[0].up.is_empty());
        assert_eq!(rows[0].down, [seg(0, 0, 0)]);
        assert!(rows[2].down.is_empty());
        assert_eq!(rows.iter().map(|r| r.color).collect::<HashSet<_>>().len(), 1);
    }

    // src/features/vcs/__tests__/graph.test.ts › computeGraph › opens a lane for a merge's second parent and joins it back
    #[test]
    fn opens_a_lane_for_a_merges_second_parent_and_joins_it_back() {
        // m merges f into main; f branched from a.
        let rows = graph(&[c("m", &["b", "f"]), c("f", &["a"]), c("b", &["a"]), c("a", &[])]);
        let (m, f, b, a) = (&rows[0], &rows[1], &rows[2], &rows[3]);
        assert_eq!(m.lane, 0);
        assert_eq!(m.down, [seg(0, 0, m.color), seg(0, 1, f.color)]);
        assert_eq!(f.lane, 1);
        assert_ne!(f.color, m.color);
        // b passes lane 1 (f's parent a) through while sitting in lane 0.
        assert_eq!(b.lane, 0);
        assert!(b.down.contains(&seg(1, 1, f.color)));
        // a is expected by both lanes: both lines converge into its node.
        assert_eq!(a.lane, 0);
        assert_eq!(a.up, [seg(0, 0, m.color), seg(1, 0, f.color)]);
        assert_eq!(a.width, 2);
    }

    // src/features/vcs/__tests__/graph.test.ts › computeGraph › reuses freed lanes and continues across pages
    #[test]
    fn reuses_freed_lanes_and_continues_across_pages() {
        let first = compute_graph(&[c("t1", &["x"]), c("t2", &["y"])], GraphState::default());
        assert_eq!(first.rows.iter().map(|r| r.lane).collect::<Vec<_>>(), [0, 1]);
        let next = compute_graph(&[c("x", &[]), c("y", &[]), c("z", &[])], first.state);
        assert_eq!(next.rows.iter().map(|r| r.lane).collect::<Vec<_>>(), [0, 1, 0]);
        assert!(next.state.lanes.is_empty());
    }

    // src/features/vcs/__tests__/graph.test.ts › computeGraph › converges lanes that lead to the same parent at its node
    #[test]
    fn converges_lanes_that_lead_to_the_same_parent_at_its_node() {
        let rows = graph(&[c("t1", &["p"]), c("t2", &["p"]), c("p", &[])]);
        assert_eq!(rows[1].lane, 1);
        assert!(rows[1].down.contains(&seg(1, 1, rows[1].color)));
        assert_eq!(rows[2].lane, 0);
        assert_eq!(rows[2].up, [seg(0, 0, rows[0].color), seg(1, 0, rows[1].color)]);
    }

    // src/features/vcs/__tests__/graph.test.ts › computeGraph › joins a merge parent that is already on another lane
    #[test]
    fn joins_a_merge_parent_that_is_already_on_another_lane() {
        // m2 merges x, which lane 1 (from t) already leads to.
        let rows = graph(&[c("t", &["x"]), c("m2", &["b", "x"]), c("x", &["b"]), c("b", &[])]);
        let m2 = &rows[1];
        assert!(m2.down.contains(&seg(m2.lane, 0, rows[0].color)));
    }

    // src/features/vcs/__tests__/graph.test.ts › straightenLaneChanges › draws a lane change as one diagonal across the row boundary, keeping the lanes
    #[test]
    fn draws_a_lane_change_as_one_diagonal_across_the_row_boundary_keeping_the_lanes() {
        // m merges f into main; f branched from a; b sits in lane 0 while lane 1 passes it.
        let laid = graph(&[c("m", &["b", "f"]), c("f", &["a"]), c("b", &["a"]), c("a", &[])]);
        let rows = straighten_lane_changes(&laid);
        let (m, f, b, a) = (&rows[0], &rows[1], &rows[2], &rows[3]);
        let drawn = |s: &GraphSegment| (s.from, s.to, s.from_x, s.to_x);
        // m → f: leaves m's node towards lane 1 and reaches f's node in a straight line.
        assert_eq!(drawn(&m.down[1]), (0, 1, None, Some(0.5)));
        assert_eq!(f.up.iter().find(|s| s.from == 1).map(drawn), Some((1, 1, Some(0.5), None)));
        // Lane 1 converging into a: the vertical line of b's row meets it halfway.
        assert_eq!(drawn(&a.up[1]), (1, 0, Some(0.5), None));
        assert_eq!(b.down.iter().find(|s| s.from == 1).map(drawn), Some((1, 1, None, Some(0.5))));
        // Straight lines stay where they are.
        assert_eq!(m.down[0].to_x, None);
        assert_eq!(a.up[0].from_x, None);
    }

    // src/features/vcs/__tests__/graph.test.ts › straightenLaneChanges › leaves a lane that another line also ends in, and arrows, as they are
    #[test]
    fn leaves_a_lane_that_another_line_also_ends_in_and_arrows_as_they_are() {
        let arrow = GraphSegment { arrow: Some(Arrow::Down), ..seg(1, 2, 1) };
        let rows = straighten_lane_changes(&[
            GraphRow { lane: 0, color: 0, down: vec![seg(0, 1, 0), seg(1, 1, 1)], width: 2, ..GraphRow::default() },
            GraphRow { lane: 1, color: 1, up: vec![seg(1, 1, 1)], down: vec![arrow], width: 3, ..GraphRow::default() },
            GraphRow { lane: 2, color: 1, up: vec![seg(2, 2, 1)], width: 3, ..GraphRow::default() },
        ]);
        assert_eq!(rows[0].down[0].to_x, None);
        assert_eq!(rows[1].up[0].from_x, None);
        assert_eq!(rows[1].down[0].to_x, None);
        assert_eq!(rows[2].up[0].from_x, None);
    }

    fn row(lane: usize, up: &[(usize, usize)], down: &[(usize, usize)]) -> GraphRow {
        GraphRow {
            lane,
            up: up.iter().map(|&(from, to)| seg(from, to, 0)).collect(),
            down: down.iter().map(|&(from, to)| seg(from, to, 0)).collect(),
            width: 9,
            ..GraphRow::default()
        }
    }

    fn ends(segments: &[GraphSegment]) -> Vec<(usize, usize)> {
        segments.iter().map(|s| (s.from, s.to)).collect()
    }

    // src/features/vcs/__tests__/graph.test.ts › compactLanes › packs the lanes of each row side by side, in their order
    #[test]
    fn packs_the_lanes_of_each_row_side_by_side_in_their_order() {
        // Lanes 2 and 7 (branch layout leaves the slots between them empty); 7 merges into 2.
        let rows = compact_lanes(&[
            row(7, &[], &[(2, 2), (7, 7)]),
            row(2, &[(2, 2), (7, 2)], &[(2, 2)]),
            row(2, &[(2, 2)], &[]),
        ]);
        assert_eq!(rows.iter().map(|r| r.lane).collect::<Vec<_>>(), [1, 0, 0]);
        assert_eq!(rows.iter().map(|r| r.width).collect::<Vec<_>>(), [2, 2, 1]);
        assert_eq!(ends(&rows[0].down), [(0, 0), (1, 1)]);
        assert_eq!(ends(&rows[1].up), [(0, 0), (1, 0)]);
    }

    // src/features/vcs/__tests__/graph.test.ts › compactLanes › keeps lines joined across rows when a lane is freed in between
    #[test]
    fn keeps_lines_joined_across_rows_when_a_lane_is_freed_in_between() {
        let laid = graph(&[
            commit("a", &["c"], &["a"]),
            commit("b", &["c"], &["b"]),
            commit("x", &["c"], &["x"]),
            commit("c", &[], &["main"]),
        ]);
        let rows = compact_lanes(&laid);
        for pair in rows.windows(2) {
            let below: BTreeSet<usize> = pair[0].down.iter().map(|s| s.to).collect();
            assert_eq!(pair[1].up.iter().map(|s| s.from).collect::<BTreeSet<_>>(), below);
            assert_eq!(below.last().copied(), Some(below.len() - 1));
        }
    }

    // src/features/vcs/__tests__/logGraphCorrectness.test.tsx › graph projections › preserves filter dashes even when collapse hides no additional commits
    #[test]
    fn preserves_filter_dashes_even_when_collapse_hides_no_additional_commits() {
        let tip = GraphCommit { dashed: vec![true], ..commit("tip", &["base"], &[]) };
        let collapsed = collapse_linear(&[tip, commit("base", &[], &[])], &set(&["tip"]), None);
        assert_eq!(collapsed.graph[0].dashed, [true]);
    }

    // src/features/vcs/__tests__/logGraphCorrectness.test.tsx › graph projections › keeps selected commits and collapses only the requested fragment
    #[test]
    fn keeps_selected_commits_and_collapses_only_the_requested_fragment() {
        let chain = [
            commit("tip", &["selected"], &[]),
            commit("selected", &["middle"], &[]),
            commit("middle", &["base"], &[]),
            commit("base", &[], &[]),
        ];
        let result = collapse_linear(&chain, &set(&["selected"]), Some(&set(&["selected", "middle"])));
        let shown: Vec<&str> = result.visible.iter().map(|&index| chain[index].id.as_str()).collect();
        assert_eq!(shown, ["tip", "selected", "base"]);
        assert_eq!(result.graph[1].parents, ["base"]);
        assert_eq!(result.graph[1].dashed, [true]);
    }

    // src/features/vcs/__tests__/logGraphCorrectness.test.tsx › graph projections › retains every node and lane beyond twenty-four simultaneous branches
    #[test]
    fn retains_every_node_and_lane_beyond_twenty_four_simultaneous_branches() {
        let tip = |i: usize| commit(&format!("tip{i}"), &[format!("base{i}").as_str()], &[]);
        let tips: Vec<GraphCommit> = (0..36).map(tip).collect();
        let bases: Vec<GraphCommit> = (0..36).map(|i| commit(&format!("base{i}"), &[], &[])).collect();
        let first = compute_graph(&tips, GraphState::default());
        assert_eq!(first.rows.last().map(|r| r.width), Some(36));
        let next = compute_graph(&bases, first.state);
        assert_eq!(next.rows.iter().map(|r| r.lane).collect::<Vec<_>>(), (0..36).collect::<Vec<_>>());
        assert!(next.state.lanes.is_empty());
    }

    // src/features/vcs/__tests__/logGraphCorrectness.test.tsx › graph projections › keeps a referenced branch's colour when an unrelated head is inserted ahead of it
    #[test]
    fn keeps_a_referenced_branchs_colour_when_an_unrelated_head_is_inserted_ahead_of_it() {
        let main = commit("main-tip", &["base"], &["HEAD", "main"]);
        let old = graph(&[main.clone(), commit("base", &[], &[])]).swap_remove(0);
        let with_tag = graph(&[commit("tag-tip", &[], &["tag: release"]), main, commit("base", &[], &[])]);
        assert_eq!(with_tag[1].color, old.color);
        assert_eq!(with_tag[1].lane, old.lane);
    }

    fn merged() -> Vec<GraphCommit> {
        vec![
            commit("merge", &["main", "feature"], &[]),
            commit("feature", &["feature-base"], &[]),
            commit("feature-base", &["base"], &[]),
            commit("main", &["base"], &[]),
            commit("base", &[], &[]),
        ]
    }

    // src/features/vcs/__tests__/logGraphCorrectness.test.tsx › linear merge presentation › serializes a contained incoming block and expands back to the actual graph
    #[test]
    fn serializes_a_contained_incoming_block_and_expands_back_to_the_actual_graph() {
        let commits = merged();
        let original = commits.clone();
        let linear = linearize_merges(&commits, &HashSet::new());
        assert_eq!(ids(&linear.graph), ids(&commits));
        assert_eq!(linear.graph[0].parents, ["feature"]);
        assert_eq!(linear.graph[2].parents, ["main"]);
        assert_eq!(linear.graph[2].dashed, [true]);
        assert_eq!(linear.fragments.get("feature-base\0main").map(String::as_str), Some("merge"));
        assert_eq!(parents(&linearize_merges(&commits, &set(&["merge"])).graph), parents(&commits));
        assert_eq!(commits, original);
    }

    // src/features/vcs/__tests__/logGraphCorrectness.test.tsx › linear merge presentation › linearizes shared fragments while preserving their other incoming edges
    #[test]
    fn linearizes_shared_fragments_while_preserving_their_other_incoming_edges() {
        let commits = merged();
        let mut shared = vec![commit("other", &["feature"], &[])];
        shared.extend(commits.iter().cloned());
        let linear = linearize_merges(&shared, &HashSet::new());
        assert_eq!(linear.graph[1].parents, ["feature"]);
        assert_eq!(linear.graph[0].parents, ["feature"]);
        assert_eq!(linearize_merges(&commits[..3], &HashSet::new()).graph[0].parents, ["main", "feature"]);
    }

    // src/features/vcs/__tests__/logGraphCorrectness.test.tsx › linear merge presentation › expands nested merge fragments together with their containing merge
    #[test]
    fn expands_nested_merge_fragments_together_with_their_containing_merge() {
        let nested = [
            commit("outer", &["main", "nested"], &[]),
            commit("nested", &["feature", "topic"], &[]),
            commit("topic", &["base"], &[]),
            commit("feature", &["base"], &[]),
            commit("main", &["base"], &[]),
            commit("base", &[], &[]),
        ];
        let collapsed = linearize_merges(&nested, &HashSet::new());
        assert_eq!(collapsed.graph[0].parents, ["nested"]);
        assert_eq!(collapsed.graph[1].parents, ["topic"]);
        assert_eq!(parents(&linearize_merges(&nested, &set(&["outer"])).graph), parents(&nested));
    }

    // src/features/vcs/__tests__/logOptions.test.ts › Collapse Linear Branches › hides linear commits and links the remaining ones with dashed edges
    #[test]
    fn hides_linear_commits_and_links_the_remaining_ones_with_dashed_edges() {
        // m merges f3 into main; f1..f3 is a linear branch from base, b a commit on main.
        let commits = [
            commit("t", &["m"], &["main"]),
            commit("m", &["b", "f3"], &[]),
            commit("f3", &["f2"], &[]),
            commit("f2", &["f1"], &[]),
            commit("f1", &["base"], &[]),
            commit("b", &["base"], &[]),
            commit("base", &[], &[]),
        ];
        let collapsed = collapse_linear(&commits, &HashSet::new(), None);
        let shown: Vec<&str> = collapsed.visible.iter().map(|&index| commits[index].id.as_str()).collect();
        assert_eq!(shown, ["t", "m", "base"]);
        let m = collapsed.graph.iter().find(|g| g.id == "m").unwrap();
        assert_eq!(m.parents, ["base"]);
        assert_eq!(m.dashed, [true]);
        let t = collapsed.graph.iter().find(|g| g.id == "t").unwrap();
        assert_eq!(t.parents, ["m"]);
        let rows = graph(&collapsed.graph);
        assert!(rows[1].down.iter().any(|s| s.dashed));
        assert!(rows[2].up.iter().any(|s| s.dashed));
    }

    // src/features/vcs/__tests__/logOptions.test.ts › Collapse Linear Branches › keeps the dashed links of a filtered log
    #[test]
    fn keeps_the_dashed_links_of_a_filtered_log() {
        // A user filter already linked y past hidden commits.
        let commits = [
            commit("z", &["y"], &["main"]),
            GraphCommit { dashed: vec![true], ..commit("y", &["x"], &["tag: v2"]) },
            commit("x", &[], &["tag: v1"]),
        ];
        let collapsed = collapse_linear(&commits, &HashSet::new(), None);
        let y = collapsed.graph.iter().find(|g| g.id == "y").unwrap();
        assert_eq!(y.parents, ["x"]);
        assert_eq!(y.dashed, [true]);
        assert_eq!(y.edge_types, [EdgeKind::Filtered]);
        assert!(graph(&commits)[1].down.iter().any(|s| s.dashed));
    }

    // src/features/vcs/__tests__/logOptions.test.ts › Collapse Linear Branches › keeps referenced and kept commits
    #[test]
    fn keeps_referenced_and_kept_commits() {
        let shown = |commits: &[GraphCommit], keep: &[&str]| -> Vec<String> {
            collapse_linear(commits, &set(keep), None).graph.into_iter().map(|g| g.id).collect()
        };
        let commits = [
            commit("d", &["c"], &[]),
            commit("c", &["b"], &["tag: v1"]),
            commit("b", &["a"], &[]),
            commit("a", &[], &[]),
        ];
        assert_eq!(shown(&commits, &[]), ["d", "c", "a"]);
        let chain = [
            commit("e", &["d"], &[]),
            commit("d", &["c"], &[]),
            commit("c", &["b"], &[]),
            commit("b", &["a"], &[]),
            commit("a", &[], &[]),
        ];
        assert_eq!(shown(&chain, &["c"]), ["e", "c", "a"]);
        assert_eq!(shown(&chain, &[]), ["e", "a"]);
    }

    // src/features/vcs/__tests__/logOptions.test.ts › long edges › hides the middle of a long straight lane and puts arrows on the stubs
    #[test]
    fn hides_the_middle_of_a_long_straight_lane_and_puts_arrows_on_the_stubs() {
        // t's parent p is 40 rows below; the commits in between sit in lane 0.
        let mut commits = vec![commit("t", &["p"], &[])];
        for i in 0..40 {
            let next = format!("x{}", i + 1);
            let parents = if i < 39 { vec![next.as_str()] } else { Vec::new() };
            commits.push(commit(&format!("x{i}"), &parents, &[]));
        }
        commits.push(commit("p", &[], &[]));
        let rows = graph(&commits);
        let hidden = hide_long_edges(&rows, LONG_EDGE);
        // The x chain sits in lane 1; the edge t → p passes through lane 0.
        let lane0 = |r: &GraphRow| r.up.iter().chain(&r.down).filter(|s| s.from == 0 && s.to == 0).count();
        assert_eq!(lane0(&rows[20]), 2);
        assert_eq!(lane0(&hidden[20]), 0);
        assert_eq!(hidden[20].lane, 1);
        assert!(hidden.iter().any(|r| r.down.iter().any(|s| s.arrow == Some(Arrow::Down))));
        assert!(hidden.iter().any(|r| r.up.iter().any(|s| s.arrow == Some(Arrow::Up))));
        assert_eq!(hide_long_edges(&rows[..10], LONG_EDGE), rows[..10]);
    }

    // src/features/vcs/__tests__/logOptions.test.ts › highlighting › finds the loaded commits of the current branch
    #[test]
    fn finds_the_loaded_commits_of_the_current_branch() {
        let commits =
            [commit("s", &["a"], &[]), commit("h", &["a"], &[]), commit("a", &["r"], &[]), commit("r", &[], &[])];
        let reached = |heads: &[&'static str]| {
            let mut out: Vec<&str> = reachable_from(&commits, heads.iter().copied()).into_iter().collect();
            out.sort_unstable();
            out
        };
        assert_eq!(reached(&["h"]), ["a", "h", "r"]);
        assert_eq!(reached(&["h", "s"]), ["a", "h", "r", "s"]);
        assert!(reached(&[]).is_empty());
    }

    // Not from a spec: the colour tokens lanes cycle through.
    #[test]
    fn cycles_the_lane_colours() {
        assert_eq!(lane_color(0), "var(--accent)");
        assert_eq!(lane_color(9), "var(--vcs-added)");
        assert_eq!(lane_color(u32::MAX), LANE_COLORS[7]);
    }
}
