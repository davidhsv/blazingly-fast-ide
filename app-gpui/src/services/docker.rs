//! Docker as a Services provider: one root per connection (containers grouped by Compose project,
//! Dev Containers, containers, images, networks, volumes) and one per registry.
//!
//! The nodes of a connection come from its last [`DockerSnapshot`] ([`children_of`],
//! [`docker_items`]). [`DockerStore`] keeps the connections and registries, each connection's
//! snapshot and the filters, and drives the backend: a snapshot is re-read on `docker events` while
//! the daemon runs and polled while it does not; lifecycle operations, pulls, prunes and Compose
//! operations run one long operation at a time. What it asks for is a [`DockerRequest`] carrying a
//! [`Ticket`]; an answer whose ticket is no longer the current one is dropped.

use crate::platform::collate;
use crate::platform::i18n::{I18n, Param};
use crate::run::store::StorageWrite;
use crate::services::model::{ItemAction, NodeCommand, ServiceMenuEntry, ServiceNode};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_docker::{
    ComposeOp, ContainerOp, DockerAvailability, DockerConnection, DockerContainer, DockerEndpoint, DockerImage,
    DockerNetwork, DockerObject, DockerRegistry, DockerSettings, DockerSnapshot, DockerVolume, PruneTarget, short_id,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Id of the Docker provider.
pub const DOCKER_PROVIDER: &str = "docker";
/// Key prefix of a registry's node; the registry id follows.
pub const REGISTRY_PREFIX: &str = "docker-registry:";
/// Key prefix of a connection's nodes; the connection id follows.
pub const CONNECTION_PREFIX: &str = "docker:";
/// Where the filters are kept.
pub const FILTERS_KEY: &str = "openidea.docker.filters";
/// How often a connection whose daemon is down (or whose events stream ended) is read again.
pub const POLL_MS: u64 = 10_000;
/// How long a burst of events rests before the snapshot is read again.
pub const REFRESH_DELAY_MS: u64 = 250;

// ---------------------------------------------------------------------------------------------------
// Filters

/// What the tree leaves out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DockerFilters {
    pub show_stopped_containers: bool,
    pub show_untagged_images: bool,
}

impl Default for DockerFilters {
    fn default() -> Self {
        Self { show_stopped_containers: true, show_untagged_images: true }
    }
}

/// The stored filters; the defaults for anything missing or unreadable.
pub fn read_filters(stored: Option<&str>) -> DockerFilters {
    stored.and_then(|text| serde_json::from_str(text).ok()).unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------------
// Nodes of a connection

/// A group below a connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GroupKind {
    DevContainers,
    Containers,
    Images,
    Networks,
    Volumes,
}

/// The groups in display order.
pub const GROUPS: [GroupKind; 5] =
    [GroupKind::DevContainers, GroupKind::Containers, GroupKind::Images, GroupKind::Networks, GroupKind::Volumes];

impl GroupKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::DevContainers => "devContainers",
            Self::Containers => "containers",
            Self::Images => "images",
            Self::Networks => "networks",
            Self::Volumes => "volumes",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::DevContainers => "Dev Containers",
            Self::Containers => "Containers",
            Self::Images => "Images",
            Self::Networks => "Networks",
            Self::Volumes => "Volumes",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::DevContainers => "docker-devcontainer",
            Self::Containers => "docker-containers",
            Self::Images => "docker-images",
            Self::Networks => "docker-networks",
            Self::Volumes => "docker-volumes",
        }
    }
}

/// A node of one connection's subtree. Its key is local to the connection ([`tree_key`]).
#[derive(Clone, Debug, PartialEq)]
pub enum DockerNode {
    /// The connection itself.
    Docker,
    Group {
        group: GroupKind,
        count: usize,
    },
    Compose {
        project: String,
        containers: Vec<DockerContainer>,
    },
    Container(Box<DockerContainer>),
    Image(DockerImage),
    Volume(DockerVolume),
    Network(DockerNetwork),
}

/// Key of the connection node, local to it.
pub const DOCKER_KEY: &str = "docker";

impl DockerNode {
    pub fn key(&self) -> String {
        match self {
            Self::Docker => DOCKER_KEY.to_string(),
            Self::Group { group, .. } => format!("group:{}", group.id()),
            Self::Compose { project, .. } => format!("compose:{project}"),
            Self::Container(container) => format!("container:{}", container.id),
            Self::Image(image) => format!("image:{}", image.id),
            Self::Volume(volume) => format!("volume:{}", volume.name),
            Self::Network(network) => format!("network:{}", network.id),
        }
    }

    /// The object docker knows the node as, with its id: `None` for the connection, groups and
    /// Compose projects.
    pub fn object(&self) -> Option<(DockerObject, &str)> {
        match self {
            Self::Container(container) => Some((DockerObject::Container, &container.id)),
            Self::Image(image) => Some((DockerObject::Image, &image.id)),
            Self::Volume(volume) => Some((DockerObject::Volume, &volume.name)),
            Self::Network(network) => Some((DockerObject::Network, &network.id)),
            Self::Docker | Self::Group { .. } | Self::Compose { .. } => None,
        }
    }
}

pub fn is_running(container: &DockerContainer) -> bool {
    container.running()
}

/// Whether the container was made by a Dev Containers tool (VS Code, IntelliJ, the CLI).
pub fn is_dev_container(container: &DockerContainer) -> bool {
    ["devcontainer.local_folder", "com.intellij.devcontainer.id", "vsch.local.folder"]
        .iter()
        .any(|label| container.labels.contains_key(*label))
}

/// Whether the image has neither a repository nor a tag (dangling).
pub fn is_untagged_image(image: &DockerImage) -> bool {
    let none = |value: &str| value.is_empty() || value == "<none>";
    none(&image.repository) && none(&image.tag)
}

/// The connection's root node.
pub fn root_node() -> DockerNode {
    DockerNode::Docker
}

fn shows_container(container: &DockerContainer, filters: DockerFilters) -> bool {
    filters.show_stopped_containers || is_running(container)
}

fn compose_projects(snapshot: &DockerSnapshot, filters: DockerFilters) -> Vec<DockerNode> {
    let mut projects: BTreeMap<&str, Vec<DockerContainer>> = BTreeMap::new();
    for container in &snapshot.containers {
        if let Some(project) = container.compose_project.as_deref() {
            projects.entry(project).or_default().push(container.clone());
        }
    }
    let mut out: Vec<(&str, Vec<DockerContainer>)> = projects
        .into_iter()
        .filter(|(_, containers)| filters.show_stopped_containers || containers.iter().any(is_running))
        .collect();
    out.sort_by(|a, b| collate::compare(a.0, b.0));
    fn service(container: &DockerContainer) -> &str {
        container.compose_service.as_deref().unwrap_or(&container.name)
    }
    out.into_iter()
        .map(|(project, mut containers)| {
            containers.sort_by(|a, b| collate::compare(service(a), service(b)));
            DockerNode::Compose { project: project.to_string(), containers }
        })
        .collect()
}

fn group_count(snapshot: &DockerSnapshot, group: GroupKind, filters: DockerFilters) -> usize {
    match group {
        GroupKind::DevContainers | GroupKind::Containers => {
            snapshot.containers.iter().filter(|c| in_container_group(c, group) && shows_container(c, filters)).count()
        }
        GroupKind::Images => {
            snapshot.images.iter().filter(|image| filters.show_untagged_images || !is_untagged_image(image)).count()
        }
        GroupKind::Volumes => snapshot.volumes.len(),
        GroupKind::Networks => snapshot.networks.len(),
    }
}

/// Whether a container outside Compose belongs to the Dev Containers or the Containers group.
fn in_container_group(container: &DockerContainer, group: GroupKind) -> bool {
    container.compose_project.is_none() && is_dev_container(container) == (group == GroupKind::DevContainers)
}

/// The nodes below one group.
pub fn group_children(snapshot: &DockerSnapshot, group: GroupKind, filters: DockerFilters) -> Vec<DockerNode> {
    match group {
        GroupKind::DevContainers | GroupKind::Containers => snapshot
            .containers
            .iter()
            .filter(|c| in_container_group(c, group) && shows_container(c, filters))
            .map(|container| DockerNode::Container(Box::new(container.clone())))
            .collect(),
        GroupKind::Images => snapshot
            .images
            .iter()
            .filter(|image| filters.show_untagged_images || !is_untagged_image(image))
            .cloned()
            .map(DockerNode::Image)
            .collect(),
        GroupKind::Volumes => snapshot.volumes.iter().cloned().map(DockerNode::Volume).collect(),
        GroupKind::Networks => snapshot.networks.iter().cloned().map(DockerNode::Network).collect(),
    }
}

/// The nodes below `node`, `None` for a leaf and for everything while the daemon cannot be used.
pub fn children_of(
    snapshot: Option<&DockerSnapshot>,
    node: &DockerNode,
    filters: DockerFilters,
) -> Option<Vec<DockerNode>> {
    let snapshot = snapshot.filter(|snapshot| matches!(snapshot.availability, DockerAvailability::Ready { .. }))?;
    match node {
        DockerNode::Docker => {
            let mut out = compose_projects(snapshot, filters);
            let groups = GROUPS.map(|group| DockerNode::Group { group, count: group_count(snapshot, group, filters) });
            out.extend(groups);
            Some(out)
        }
        DockerNode::Group { group, .. } => Some(group_children(snapshot, *group, filters)),
        DockerNode::Compose { containers, .. } => Some(
            containers
                .iter()
                .filter(|container| shows_container(container, filters))
                .map(|container| DockerNode::Container(Box::new(container.clone())))
                .collect(),
        ),
        _ => None,
    }
}

/// The node with the local `key` in `snapshot` (unfiltered), so a selection survives refreshes.
pub fn find_node(snapshot: Option<&DockerSnapshot>, key: &str) -> Option<DockerNode> {
    fn walk(snapshot: Option<&DockerSnapshot>, nodes: Vec<DockerNode>, key: &str) -> Option<DockerNode> {
        for node in nodes {
            if node.key() == key {
                return Some(node);
            }
            let children = children_of(snapshot, &node, DockerFilters::default()).unwrap_or_default();
            if let Some(found) = walk(snapshot, children, key) {
                return Some(found);
            }
        }
        None
    }
    walk(snapshot, vec![root_node()], key)
}

pub fn node_label(node: &DockerNode) -> String {
    match node {
        DockerNode::Docker => "Docker".to_string(),
        DockerNode::Group { group, .. } => group.label().to_string(),
        DockerNode::Compose { project, .. } => format!("Docker-compose: {project}"),
        DockerNode::Container(container) => match (&container.compose_service, &container.compose_project) {
            (Some(service), Some(_)) => service.clone(),
            _ => container.name.clone(),
        },
        DockerNode::Image(image) => image.reference(),
        DockerNode::Volume(volume) => volume.name.clone(),
        DockerNode::Network(network) => network.name.clone(),
    }
}

/// The grey text after a node's label (English; the view translates it).
pub fn node_detail(node: &DockerNode) -> Option<String> {
    match node {
        DockerNode::Docker => None,
        DockerNode::Group { count, .. } => (*count > 0).then(|| count.to_string()),
        DockerNode::Compose { containers, .. } => {
            let up = containers.iter().filter(|container| is_running(container)).count();
            Some(format!("{up}/{} running", containers.len()))
        }
        DockerNode::Container(container) => Some(match &container.compose_service {
            Some(_) => format!("{} · {}", container.status, container.name),
            None => format!("{} · {}", container.status, container.image),
        }),
        DockerNode::Image(image) => Some(format!("{} · {}", image.size, image.created_since)),
        DockerNode::Volume(volume) => Some(volume.driver.clone()),
        DockerNode::Network(network) => Some(network.driver.clone()),
    }
}

pub fn node_icon(node: &DockerNode) -> &'static str {
    match node {
        DockerNode::Docker => "docker-connected",
        DockerNode::Group { group, .. } => group.icon(),
        DockerNode::Compose { containers, .. } if containers.iter().any(is_running) => "docker-compose-running",
        DockerNode::Compose { .. } => "docker-compose-stopped",
        DockerNode::Container(container) if is_dev_container(container) => "docker-devcontainer",
        DockerNode::Container(container) if container.compose_project.is_some() => {
            if is_running(container) {
                "docker-compose-service-running"
            } else {
                "docker-compose-service-stopped"
            }
        }
        DockerNode::Container(container) if container.state == "paused" => "pause",
        DockerNode::Container(container) if is_running(container) => "docker-container-running",
        DockerNode::Container(_) => "docker-container-stopped",
        DockerNode::Image(_) => "docker-image",
        DockerNode::Volume(_) => "docker-volumes",
        DockerNode::Network(_) => "docker-networks",
    }
}

/// The containers a node stands for: a Compose project's, or the container itself.
pub fn node_containers(node: &DockerNode) -> Vec<&DockerContainer> {
    match node {
        DockerNode::Container(container) => vec![container.as_ref()],
        DockerNode::Compose { containers, .. } => containers.iter().collect(),
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------------------------------
// Cross references

/// Whether the container's image reference names `image`.
pub fn container_matches_image(container: &DockerContainer, image: &DockerImage) -> bool {
    let target = container.image.as_str();
    if target == image.reference() || target == image.id || target == short_id(&image.id) {
        return true;
    }
    if image.tag == "latest" && target == image.repository {
        return true;
    }
    let clean_id = image.id.strip_prefix("sha256:").unwrap_or(&image.id);
    let clean_target = target.strip_prefix("sha256:").unwrap_or(target);
    clean_target.len() >= 12 && clean_id.starts_with(clean_target)
}

pub fn containers_using_image<'a>(snapshot: &'a DockerSnapshot, image: &DockerImage) -> Vec<&'a DockerContainer> {
    snapshot.containers.iter().filter(|container| container_matches_image(container, image)).collect()
}

/// Every `repo:tag` sharing the image's id, each once.
pub fn image_tags_for_id(snapshot: &DockerSnapshot, image: &DockerImage) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for other in snapshot.images.iter().filter(|other| other.id == image.id && !is_untagged_image(other)) {
        let tag = other.reference();
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    tags
}

/// The local image the container runs.
pub fn find_image_for_container<'a>(
    snapshot: &'a DockerSnapshot,
    container: &DockerContainer,
) -> Option<&'a DockerImage> {
    snapshot.images.iter().find(|image| container_matches_image(container, image))
}

pub fn containers_using_volume<'a>(snapshot: &'a DockerSnapshot, volume: &DockerVolume) -> Vec<&'a DockerContainer> {
    snapshot.containers.iter().filter(|container| container.mounts.contains(&volume.name)).collect()
}

pub fn containers_in_network<'a>(snapshot: &'a DockerSnapshot, network: &DockerNetwork) -> Vec<&'a DockerContainer> {
    snapshot.containers.iter().filter(|container| container.networks.contains(&network.name)).collect()
}

// ---------------------------------------------------------------------------------------------------
// Tree keys

pub fn connection_key(conn: &str) -> String {
    format!("{CONNECTION_PREFIX}{conn}")
}

pub fn registry_key(id: &str) -> String {
    format!("{REGISTRY_PREFIX}{id}")
}

/// The tree key of `node` of connection `conn`.
pub fn tree_key(conn: &str, node: &DockerNode) -> String {
    match node {
        DockerNode::Docker => connection_key(conn),
        _ => format!("{}:{}", connection_key(conn), node.key()),
    }
}

/// A tree key split into the connection id and the key local to it; `None` for other providers'
/// keys and for registries.
pub fn parse_tree_key(key: &str) -> Option<(&str, &str)> {
    let rest = key.strip_prefix(CONNECTION_PREFIX)?;
    Some(rest.split_once(':').unwrap_or((rest, DOCKER_KEY)))
}

/// A registry's id from its tree key.
pub fn parse_registry_key(key: &str) -> Option<&str> {
    key.strip_prefix(REGISTRY_PREFIX)
}

// ---------------------------------------------------------------------------------------------------
// The provider's roots

/// What is known of one connection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConnectionState {
    pub snapshot: Option<DockerSnapshot>,
    pub loading: bool,
    pub error: Option<String>,
}

impl ConnectionState {
    pub fn is_ready(&self) -> bool {
        self.snapshot.as_ref().is_some_and(|snapshot| matches!(snapshot.availability, DockerAvailability::Ready { .. }))
    }
}

/// The grey text after a connection's name.
fn connection_detail(state: &ConnectionState) -> Option<String> {
    let Some(snapshot) = &state.snapshot else {
        return state.error.clone().or_else(|| state.loading.then(|| "connecting…".to_string()));
    };
    match &snapshot.availability {
        DockerAvailability::NotInstalled => Some("docker not installed".to_string()),
        DockerAvailability::DaemonDown { .. } => Some("not running".to_string()),
        DockerAvailability::Ready { context, .. } => context.clone(),
    }
}

/// The Docker part of the Services tree: every connection, then every registry.
pub fn docker_items(
    connections: &[DockerConnection],
    registries: &[DockerRegistry],
    states: &BTreeMap<String, ConnectionState>,
    filters: DockerFilters,
) -> Vec<ServiceNode> {
    fn convert(
        conn: &str,
        snapshot: Option<&DockerSnapshot>,
        node: &DockerNode,
        filters: DockerFilters,
    ) -> ServiceNode {
        let children = children_of(snapshot, node, filters);
        ServiceNode {
            key: tree_key(conn, node),
            label: node_label(node),
            icon: node_icon(node).to_string(),
            detail: node_detail(node),
            dim: matches!(node, DockerNode::Container(container) if !is_running(container)),
            expanded: false,
            expandable: false,
            children: children.map(|kids| kids.iter().map(|kid| convert(conn, snapshot, kid, filters)).collect()),
        }
    }
    let idle = ConnectionState::default();
    let mut items: Vec<ServiceNode> = connections
        .iter()
        .map(|connection| {
            let state = states.get(&connection.id).unwrap_or(&idle);
            let snapshot = state.snapshot.as_ref();
            let children = children_of(snapshot, &root_node(), filters).unwrap_or_default();
            ServiceNode {
                key: connection_key(&connection.id),
                label: connection.name.clone(),
                icon: if state.is_ready() { "docker-connected" } else { "file-docker" }.to_string(),
                detail: connection_detail(state),
                dim: !state.is_ready(),
                expanded: false,
                expandable: false,
                children: Some(children.iter().map(|kid| convert(&connection.id, snapshot, kid, filters)).collect()),
            }
        })
        .collect();
    for registry in registries {
        items.push(ServiceNode {
            detail: Some(registry.address.clone()),
            ..ServiceNode::leaf(registry_key(&registry.id), &registry.name, "docker-registry")
        });
    }
    items
}

/// A node of a connection's subtree.
#[derive(Clone, Debug, PartialEq)]
pub struct ConnectionNode {
    pub conn: String,
    pub node: DockerNode,
}

pub fn can_start(target: &ConnectionNode) -> bool {
    node_containers(&target.node).iter().any(|container| !is_running(container))
}

pub fn can_stop(target: &ConnectionNode) -> bool {
    node_containers(&target.node).iter().any(|container| is_running(container))
}

pub fn can_restart(target: &ConnectionNode) -> bool {
    matches!(target.node, DockerNode::Container(_) | DockerNode::Compose { .. })
}

pub fn can_remove(target: &ConnectionNode) -> bool {
    matches!(
        target.node,
        DockerNode::Container(_)
            | DockerNode::Compose { .. }
            | DockerNode::Image(_)
            | DockerNode::Volume(_)
            | DockerNode::Network(_)
    )
}

// ---------------------------------------------------------------------------------------------------
// Detail tabs, questions and commands

/// The tab the detail pane of a Docker object shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DetailTab {
    #[default]
    Dashboard,
    Log,
    Processes,
    Layers,
    Inspect,
}

impl DetailTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Dashboard => "Dashboard",
            Self::Log => "Log",
            Self::Processes => "Processes",
            Self::Layers => "Layers",
            Self::Inspect => "Inspect",
        }
    }
}

/// The tabs the detail pane of `node` offers.
pub fn detail_tabs(node: &DockerNode) -> Vec<DetailTab> {
    use DetailTab::{Dashboard, Inspect, Layers, Log, Processes};
    match node {
        DockerNode::Container(_) => vec![Dashboard, Log, Processes, Inspect],
        DockerNode::Image(_) => vec![Dashboard, Layers, Inspect],
        DockerNode::Volume(_) | DockerNode::Network(_) => vec![Dashboard, Inspect],
        DockerNode::Docker | DockerNode::Group { .. } | DockerNode::Compose { .. } => Vec::new(),
    }
}

/// What deleting a node does once the user agrees: remove objects, or take a Compose project down.
#[derive(Clone, Debug, PartialEq)]
pub enum Deletion {
    Objects { object: DockerObject, ids: Vec<String>, running: bool },
    ComposeDown { project: String, files: Vec<String>, working_dir: Option<String> },
}

/// A confirmation to ask before something is deleted.
#[derive(Clone, Debug, PartialEq)]
pub struct DeleteQuestion {
    pub conn: String,
    pub title: String,
    pub message: String,
    pub ok: String,
    /// A checkbox offered with the question (an image: force).
    pub checkbox: Option<String>,
    pub deletion: Deletion,
}

/// A confirmation to ask before a prune.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PruneQuestion {
    pub title: String,
    pub message: String,
    pub ok: String,
}

/// Prune targets with their menu labels and questions.
const PRUNE: [(PruneTarget, &str, &str); 6] = [
    (PruneTarget::Containers, "Stopped Containers", "Remove all stopped containers?"),
    (PruneTarget::Images, "Dangling Images", "Remove all dangling (untagged) images?"),
    (PruneTarget::AllImages, "Unused Images", "Remove all images not used by a container?"),
    (
        PruneTarget::Volumes,
        "Unused Volumes",
        "Remove all anonymous volumes not used by a container? Their data is lost.",
    ),
    (PruneTarget::Networks, "Unused Networks", "Remove all networks not used by a container?"),
    (
        PruneTarget::System,
        "Everything Unused (System Prune)",
        "Remove stopped containers, unused networks, dangling images and the build cache?",
    ),
];

/// The question asked before pruning `target`.
pub fn prune_question(target: PruneTarget, i18n: &mut I18n) -> PruneQuestion {
    let (_, label, message) = PRUNE.iter().find(|(known, _, _)| *known == target).copied().unwrap_or(PRUNE[5]);
    PruneQuestion {
        title: i18n.tx_built(&["Prune ", ""], &[Param::from(label)]),
        message: i18n.tx(message),
        ok: i18n.tx("Prune"),
    }
}

fn prune_label(target: PruneTarget) -> &'static str {
    match target {
        PruneTarget::Containers => "Prune Unused Containers…",
        PruneTarget::Images => "Prune Dangling Images…",
        PruneTarget::AllImages => "Prune Unused Images…",
        PruneTarget::Volumes => "Prune Unused Volumes…",
        PruneTarget::Networks => "Prune Unused Networks…",
        PruneTarget::System => "Prune Everything Unused…",
    }
}

/// What a Docker toolbar button or menu entry asks for. Commands naming a node by its tree key are
/// resolved against the snapshot when they are carried out ([`DockerStore::carry_out`]).
#[derive(Clone, Debug, PartialEq)]
pub enum DockerCommand {
    Refresh {
        conn: String,
    },
    Start {
        key: String,
    },
    Stop {
        key: String,
    },
    Restart {
        key: String,
    },
    Pause {
        key: String,
    },
    Unpause {
        key: String,
    },
    /// Ask, then delete the node (Delete…, Compose Down).
    Delete {
        key: String,
    },
    /// A shell in the running container, in a terminal tab.
    Terminal {
        key: String,
    },
    /// Ask for an image reference, then pull it.
    PullPrompt {
        conn: String,
    },
    Pull {
        conn: String,
        reference: String,
    },
    /// Ask, then prune.
    PrunePrompt {
        conn: String,
        target: PruneTarget,
    },
    ShowDetail(DetailTab),
    OpenLogInEditor {
        key: String,
    },
    Copy(String),
    ToggleStoppedContainers,
    ToggleUntaggedImages,
    AddConnection,
    EditConnection(String),
    RemoveConnection(String),
    AddRegistry,
    EditRegistry(String),
    TestLogin(String),
    RemoveRegistry(String),
    /// Cancel on the long operation's banner.
    CancelTask,
}

fn docker(command: DockerCommand) -> Option<NodeCommand> {
    Some(NodeCommand::Docker(command))
}

// ---------------------------------------------------------------------------------------------------
// Requests and answers

/// Identifies one request; an answer carrying an older ticket than the current one is dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ticket(pub u64);

/// Why an operation ended without success.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpError {
    /// The user cancelled it: nothing is reported.
    Cancelled,
    Failed(String),
}

/// A streaming operation: its output lines go to [`DockerStore::task_line`].
#[derive(Clone, Debug, PartialEq)]
pub enum StreamOp {
    /// `ide_docker::pull`, logging in with the saved registry matching the reference first
    /// (`ide_docker::auth_for`).
    Pull { reference: String },
    /// `ide_docker::prune`.
    Prune(PruneTarget),
    /// `ide_docker::compose_op`.
    Compose { project: String, files: Vec<String>, working_dir: Option<String>, op: ComposeOp },
}

/// What the caller carries out for the store, or asks the user. Each backend request is answered
/// through the method its documentation names.
#[derive(Clone, Debug, PartialEq)]
pub enum DockerRequest {
    /// `ide_docker::load_settings`, then [`DockerStore::settings_loaded`].
    LoadSettings {
        ticket: Ticket,
    },
    /// `ide_docker::save_settings`, then [`DockerStore::settings_saved`].
    SaveSettings {
        ticket: Ticket,
        settings: Box<DockerSettings>,
    },
    /// `ide_docker::set_registry_password` (`None` forgets it), then [`DockerStore::password_saved`].
    SetRegistryPassword {
        ticket: Ticket,
        id: String,
        password: Option<String>,
    },
    /// `ide_docker::snapshot`, then [`DockerStore::snapshot_answered`].
    Snapshot {
        ticket: Ticket,
        conn: String,
        endpoint: DockerEndpoint,
    },
    /// `ide_docker::watch_events` with a cancellation token kept under the ticket: each event goes to
    /// [`DockerStore::event`], the end of the stream to [`DockerStore::events_ended`].
    WatchEvents {
        ticket: Ticket,
        conn: String,
        endpoint: DockerEndpoint,
    },
    /// Cancel the stream or operation of this ticket.
    Cancel {
        ticket: Ticket,
    },
    /// Wait, then [`DockerStore::refresh_due`].
    RefreshLater {
        ticket: Ticket,
        delay_ms: u64,
    },
    /// `ide_docker::container_op`, then [`DockerStore::operation_done`].
    ContainerOp {
        ticket: Ticket,
        endpoint: DockerEndpoint,
        ids: Vec<String>,
        op: ContainerOp,
    },
    /// `ide_docker::remove`, then [`DockerStore::operation_done`].
    Remove {
        ticket: Ticket,
        endpoint: DockerEndpoint,
        object: DockerObject,
        ids: Vec<String>,
        force: bool,
    },
    /// A streaming operation, then [`DockerStore::task_done`].
    Stream {
        ticket: Ticket,
        endpoint: DockerEndpoint,
        op: StreamOp,
    },
    /// `ide_docker::login` with the registry's saved password, then [`DockerStore::login_answered`].
    RegistryLogin {
        ticket: Ticket,
        endpoint: DockerEndpoint,
        registry: DockerRegistry,
    },
    /// `ide_docker::inspect`, then [`DockerStore::inspect_answered`].
    Inspect {
        ticket: Ticket,
        endpoint: DockerEndpoint,
        object: DockerObject,
        id: String,
    },
    Notify(Box<NotificationRequest>),
    /// Write to the UI storage.
    Store(StorageWrite),
    /// Ask the question; if agreed, [`DockerStore::delete_confirmed`].
    ConfirmDelete(Box<DeleteQuestion>),
    /// Ask for an image reference; then [`DockerCommand::Pull`].
    PromptPull {
        conn: String,
    },
    /// Ask the question; if agreed, [`DockerStore::prune`].
    ConfirmPrune {
        conn: String,
        target: PruneTarget,
        question: PruneQuestion,
    },
    /// Ask before removing the connection; if agreed, [`DockerStore::remove_connection`].
    ConfirmRemoveConnection {
        id: String,
        title: String,
        message: String,
    },
    /// Ask before removing the registry; if agreed, [`DockerStore::remove_registry`].
    ConfirmRemoveRegistry {
        id: String,
        title: String,
        message: String,
    },
    /// Open the connection dialog (`None`: a new connection); its answer goes to
    /// [`DockerStore::save_connection`].
    ConnectionDialog(Option<DockerConnection>),
    /// Open the registry dialog (`None`: a new registry); its answer goes to
    /// [`DockerStore::save_registry`].
    RegistryDialog(Option<DockerRegistry>),
    /// Run `program` with `args` in a new terminal tab named `name` (`terminal.runCommand`).
    OpenTerminal {
        program: String,
        args: Vec<String>,
        name: String,
    },
    CopyText(String),
    /// Open the container's log in an editor tab.
    OpenLogInEditor {
        conn: String,
        container: Box<DockerContainer>,
    },
}

fn notify(level: NotificationLevel, title: String, body: Option<String>, key: &str) -> DockerRequest {
    let mut notification = NotificationRequest::new(level, title);
    notification.group = Some("run".to_string());
    notification.body = body;
    notification.key = Some(key.to_string());
    DockerRequest::Notify(Box::new(notification))
}

/// A change of the settings waiting for them to be saved.
#[derive(Clone, Debug, PartialEq)]
enum SettingsChange {
    SaveConnection(DockerConnection),
    RemoveConnection(String),
    /// `password`: `None` keeps the saved one, `Some(None)` forgets it.
    SaveRegistry {
        registry: DockerRegistry,
        password: Option<Option<String>>,
    },
    RemoveRegistry(String),
}

/// A lifecycle operation waiting for its answer.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingOp {
    conn: String,
    failure: String,
}

/// The long operation in progress (pull, prune, Compose): its title and last output line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockerTask {
    pub ticket: Ticket,
    pub conn: String,
    pub title: String,
    pub failure: String,
    pub line: String,
}

/// The inspect payload the detail pane shows, for the node it was asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InspectState {
    pub key: String,
    /// `None` while it loads.
    pub result: Option<Result<String, String>>,
}

/// The Docker provider's state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DockerStore {
    /// `None` until loaded.
    settings: Option<DockerSettings>,
    settings_load: Option<Ticket>,
    /// Changes made before the settings were loaded.
    queued: Vec<SettingsChange>,
    saving: BTreeMap<Ticket, (DockerSettings, SettingsChange)>,
    conns: BTreeMap<String, ConnectionState>,
    filters: DockerFilters,
    detail_tab: DetailTab,
    task: Option<DockerTask>,
    next_ticket: u64,
    /// The snapshot read in flight per connection, and the connections to read again once it ends.
    snapshots: BTreeMap<String, Ticket>,
    again: BTreeSet<String>,
    events: BTreeMap<String, Ticket>,
    debounce: BTreeMap<String, Ticket>,
    ops: BTreeMap<Ticket, PendingOp>,
    logins: BTreeMap<Ticket, String>,
    inspect: Option<(Ticket, InspectState)>,
    watchers: usize,
}

impl DockerStore {
    /// The store with the filters as stored.
    pub fn new(stored_filters: Option<&str>) -> Self {
        Self { filters: read_filters(stored_filters), ..Self::default() }
    }

    fn ticket(&mut self) -> Ticket {
        self.next_ticket += 1;
        Ticket(self.next_ticket)
    }

    pub fn settings(&self) -> Option<&DockerSettings> {
        self.settings.as_ref()
    }

    pub fn connections(&self) -> &[DockerConnection] {
        self.settings.as_ref().map(|settings| settings.connections.as_slice()).unwrap_or_default()
    }

    pub fn registries(&self) -> &[DockerRegistry] {
        self.settings.as_ref().map(|settings| settings.registries.as_slice()).unwrap_or_default()
    }

    pub fn connection_state(&self, conn: &str) -> Option<&ConnectionState> {
        self.conns.get(conn)
    }

    pub fn filters(&self) -> DockerFilters {
        self.filters
    }

    pub fn detail_tab(&self) -> DetailTab {
        self.detail_tab
    }

    pub fn task(&self) -> Option<&DockerTask> {
        self.task.as_ref()
    }

    pub fn inspect(&self) -> Option<&InspectState> {
        self.inspect.as_ref().map(|(_, state)| state)
    }

    pub fn is_watching(&self) -> bool {
        self.watchers > 0
    }

    fn endpoint_of(&self, conn: &str) -> DockerEndpoint {
        self.connections().iter().find(|known| known.id == conn).map(|known| known.endpoint.clone()).unwrap_or_default()
    }

    fn has_connection(&self, conn: &str) -> bool {
        self.connections().iter().any(|known| known.id == conn)
    }

    /// The provider's roots.
    pub fn roots(&self) -> Vec<ServiceNode> {
        docker_items(self.connections(), self.registries(), &self.conns, self.filters)
    }

    /// The connection node a tree key stands for, in the connection's last snapshot.
    pub fn resolve(&self, key: &str) -> Option<ConnectionNode> {
        let (conn, local) = parse_tree_key(key)?;
        let node = if local == DOCKER_KEY {
            root_node()
        } else {
            find_node(self.conns.get(conn).and_then(|state| state.snapshot.as_ref()), local)?
        };
        Some(ConnectionNode { conn: conn.to_string(), node })
    }

    fn registry(&self, id: &str) -> Option<&DockerRegistry> {
        self.registries().iter().find(|registry| registry.id == id)
    }

    fn connection(&self, id: &str) -> Option<&DockerConnection> {
        self.connections().iter().find(|connection| connection.id == id)
    }

    // -----------------------------------------------------------------------------------------------
    // Settings

    fn load_settings(&mut self) -> Vec<DockerRequest> {
        if self.settings.is_some() || self.settings_load.is_some() {
            return Vec::new();
        }
        let ticket = self.ticket();
        self.settings_load = Some(ticket);
        vec![DockerRequest::LoadSettings { ticket }]
    }

    /// The answer to [`DockerRequest::LoadSettings`]: queued changes are saved now, and a watched
    /// tree reads every connection.
    pub fn settings_loaded(&mut self, ticket: Ticket, loaded: Result<DockerSettings, String>) -> Vec<DockerRequest> {
        if self.settings_load != Some(ticket) {
            return Vec::new();
        }
        self.settings_load = None;
        match loaded {
            Ok(settings) => {
                self.settings = Some(settings);
                let mut out: Vec<DockerRequest> =
                    std::mem::take(&mut self.queued).into_iter().flat_map(|change| self.change(change)).collect();
                if self.watchers > 0 {
                    out.extend(self.refresh_all());
                }
                out
            }
            Err(error) => {
                vec![notify(NotificationLevel::Error, "Cannot read Docker settings".to_string(), Some(error), "docker")]
            }
        }
    }

    fn change(&mut self, change: SettingsChange) -> Vec<DockerRequest> {
        let Some(mut settings) = self.settings.clone() else {
            self.queued.push(change);
            return self.load_settings();
        };
        match &change {
            SettingsChange::SaveConnection(connection) => {
                match settings.connections.iter_mut().find(|known| known.id == connection.id) {
                    Some(known) => *known = connection.clone(),
                    None => settings.connections.push(connection.clone()),
                }
            }
            SettingsChange::RemoveConnection(id) => settings.connections.retain(|known| known.id != *id),
            SettingsChange::SaveRegistry { registry, .. } => {
                match settings.registries.iter_mut().find(|known| known.id == registry.id) {
                    Some(known) => *known = registry.clone(),
                    None => settings.registries.push(registry.clone()),
                }
            }
            SettingsChange::RemoveRegistry(id) => settings.registries.retain(|known| known.id != *id),
        }
        let mut out = Vec::new();
        if let SettingsChange::RemoveConnection(id) = &change {
            out.extend(self.stop_events(Some(id)));
        }
        let ticket = self.ticket();
        out.push(DockerRequest::SaveSettings { ticket, settings: Box::new(settings.clone()) });
        self.saving.insert(ticket, (settings, change));
        out
    }

    /// Adds or replaces (same id) a connection; the tree reads it once saved.
    pub fn save_connection(&mut self, connection: DockerConnection) -> Vec<DockerRequest> {
        self.change(SettingsChange::SaveConnection(connection))
    }

    pub fn remove_connection(&mut self, id: &str) -> Vec<DockerRequest> {
        self.change(SettingsChange::RemoveConnection(id.to_string()))
    }

    /// Adds or replaces a registry; `password` `None` keeps the saved one, `Some(None)` forgets it.
    pub fn save_registry(&mut self, registry: DockerRegistry, password: Option<Option<String>>) -> Vec<DockerRequest> {
        self.change(SettingsChange::SaveRegistry { registry, password })
    }

    pub fn remove_registry(&mut self, id: &str) -> Vec<DockerRequest> {
        self.change(SettingsChange::RemoveRegistry(id.to_string()))
    }

    /// The answer to [`DockerRequest::SaveSettings`]: the change takes effect, or is reported.
    pub fn settings_saved(&mut self, ticket: Ticket, saved: Result<(), String>) -> Vec<DockerRequest> {
        let Some((settings, change)) = self.saving.remove(&ticket) else { return Vec::new() };
        if let Err(error) = saved {
            let title = "Cannot save Docker settings".to_string();
            return vec![notify(NotificationLevel::Error, title, Some(error), "docker")];
        }
        self.settings = Some(settings);
        match change {
            SettingsChange::SaveConnection(connection) => {
                let mut out = self.stop_events(Some(&connection.id));
                if self.watchers > 0 {
                    out.extend(self.refresh(&connection.id));
                }
                out
            }
            SettingsChange::RemoveConnection(id) => {
                self.conns.remove(&id);
                self.snapshots.remove(&id);
                self.again.remove(&id);
                self.debounce.remove(&id);
                Vec::new()
            }
            SettingsChange::SaveRegistry { registry, password: Some(password) } => {
                let ticket = self.ticket();
                vec![DockerRequest::SetRegistryPassword { ticket, id: registry.id, password }]
            }
            SettingsChange::SaveRegistry { password: None, .. } | SettingsChange::RemoveRegistry(_) => Vec::new(),
        }
    }

    /// The answer to [`DockerRequest::SetRegistryPassword`].
    pub fn password_saved(&mut self, saved: Result<(), String>) -> Vec<DockerRequest> {
        match saved {
            Ok(()) => Vec::new(),
            Err(error) => {
                let title = "Cannot save the registry password".to_string();
                vec![notify(NotificationLevel::Error, title, Some(error), "docker")]
            }
        }
    }

    // -----------------------------------------------------------------------------------------------
    // Snapshots

    /// The tree is shown: keep the snapshots live. Every [`DockerStore::watch`] is paired with an
    /// [`DockerStore::unwatch`]; while watched, the caller calls [`DockerStore::poll`] every
    /// [`POLL_MS`].
    pub fn watch(&mut self) -> Vec<DockerRequest> {
        self.watchers += 1;
        match self.settings {
            None => self.load_settings(),
            Some(_) => self.refresh_all(),
        }
    }

    /// The tree is no longer shown; with no one watching, the events streams stop.
    pub fn unwatch(&mut self) -> Vec<DockerRequest> {
        self.watchers = self.watchers.saturating_sub(1);
        if self.watchers > 0 {
            return Vec::new();
        }
        self.debounce.clear();
        self.stop_events(None)
    }

    /// The poll: re-reads the connections whose daemon is down or whose events stream is not running.
    pub fn poll(&mut self) -> Vec<DockerRequest> {
        if self.watchers == 0 {
            return Vec::new();
        }
        let due: Vec<String> = self
            .connections()
            .iter()
            .filter(|connection| {
                let ready = self.conns.get(&connection.id).is_some_and(ConnectionState::is_ready);
                !ready || !self.events.contains_key(&connection.id)
            })
            .map(|connection| connection.id.clone())
            .collect();
        due.iter().flat_map(|conn| self.refresh(conn)).collect()
    }

    /// Re-reads every connection (Refresh Docker).
    pub fn refresh_all(&mut self) -> Vec<DockerRequest> {
        let ids: Vec<String> = self.connections().iter().map(|connection| connection.id.clone()).collect();
        ids.iter().flat_map(|conn| self.refresh(conn)).collect()
    }

    /// Re-reads one connection. While a read is in flight another one follows it, once.
    pub fn refresh(&mut self, conn: &str) -> Vec<DockerRequest> {
        if !self.has_connection(conn) {
            return Vec::new();
        }
        if self.snapshots.contains_key(conn) {
            self.again.insert(conn.to_string());
            return Vec::new();
        }
        let ticket = self.ticket();
        self.snapshots.insert(conn.to_string(), ticket);
        self.conns.entry(conn.to_string()).or_default().loading = true;
        vec![DockerRequest::Snapshot { ticket, conn: conn.to_string(), endpoint: self.endpoint_of(conn) }]
    }

    /// The answer to [`DockerRequest::Snapshot`]. A daemon that just became ready starts its events
    /// stream; a read asked for meanwhile follows.
    pub fn snapshot_answered(&mut self, ticket: Ticket, answer: Result<DockerSnapshot, String>) -> Vec<DockerRequest> {
        let Some(conn) = self.snapshots.iter().find(|(_, known)| **known == ticket).map(|(conn, _)| conn.clone())
        else {
            return Vec::new();
        };
        self.snapshots.remove(&conn);
        if !self.has_connection(&conn) {
            self.again.remove(&conn);
            return Vec::new();
        }
        let state = self.conns.entry(conn.clone()).or_default();
        let was_ready = state.is_ready();
        state.loading = false;
        let mut out = Vec::new();
        match answer {
            Ok(snapshot) => {
                state.snapshot = Some(snapshot);
                state.error = None;
                if state.is_ready() && !was_ready {
                    out.extend(self.start_events(&conn));
                }
            }
            Err(error) => state.error = Some(error),
        }
        if self.again.remove(&conn) {
            out.extend(self.refresh(&conn));
        }
        out
    }

    fn start_events(&mut self, conn: &str) -> Vec<DockerRequest> {
        if self.events.contains_key(conn) || self.watchers == 0 {
            return Vec::new();
        }
        let ticket = self.ticket();
        self.events.insert(conn.to_string(), ticket);
        vec![DockerRequest::WatchEvents { ticket, conn: conn.to_string(), endpoint: self.endpoint_of(conn) }]
    }

    fn stop_events(&mut self, conn: Option<&str>) -> Vec<DockerRequest> {
        let stopped: Vec<String> =
            self.events.keys().filter(|known| conn.is_none_or(|conn| conn == known.as_str())).cloned().collect();
        stopped
            .into_iter()
            .filter_map(|known| self.events.remove(&known))
            .map(|ticket| DockerRequest::Cancel { ticket })
            .collect()
    }

    fn conn_of_events(&self, ticket: Ticket) -> Option<String> {
        self.events.iter().find(|(_, known)| **known == ticket).map(|(conn, _)| conn.clone())
    }

    /// An event of [`DockerRequest::WatchEvents`]: the snapshot is read again once the burst rests.
    pub fn event(&mut self, ticket: Ticket) -> Vec<DockerRequest> {
        match self.conn_of_events(ticket) {
            Some(conn) => self.refresh_soon(&conn),
            None => Vec::new(),
        }
    }

    /// The events stream ended on its own (the daemon stopped): the state is checked again.
    pub fn events_ended(&mut self, ticket: Ticket) -> Vec<DockerRequest> {
        let Some(conn) = self.conn_of_events(ticket) else { return Vec::new() };
        self.events.remove(&conn);
        if self.watchers > 0 { self.refresh_soon(&conn) } else { Vec::new() }
    }

    fn refresh_soon(&mut self, conn: &str) -> Vec<DockerRequest> {
        let ticket = self.ticket();
        self.debounce.insert(conn.to_string(), ticket);
        vec![DockerRequest::RefreshLater { ticket, delay_ms: REFRESH_DELAY_MS }]
    }

    /// The wait of [`DockerRequest::RefreshLater`] is over: unless a later one replaced it, the
    /// connection is read again.
    pub fn refresh_due(&mut self, ticket: Ticket) -> Vec<DockerRequest> {
        let Some(conn) = self.debounce.iter().find(|(_, known)| **known == ticket).map(|(conn, _)| conn.clone()) else {
            return Vec::new();
        };
        self.debounce.remove(&conn);
        self.refresh(&conn)
    }

    // -----------------------------------------------------------------------------------------------
    // Operations

    fn operation(
        &mut self,
        conn: &str,
        failure: String,
        request: impl FnOnce(Ticket) -> DockerRequest,
    ) -> Vec<DockerRequest> {
        let ticket = self.ticket();
        self.ops.insert(ticket, PendingOp { conn: conn.to_string(), failure });
        vec![request(ticket)]
    }

    /// Starts, stops… the containers `ids` of connection `conn`.
    pub fn container_op(
        &mut self,
        conn: &str,
        ids: Vec<String>,
        op: ContainerOp,
        i18n: &mut I18n,
    ) -> Vec<DockerRequest> {
        if ids.is_empty() {
            return Vec::new();
        }
        let failure = i18n.tx(match op {
            ContainerOp::Start => "Could not start container",
            ContainerOp::Stop => "Could not stop container",
            ContainerOp::Restart => "Could not restart container",
            ContainerOp::Pause => "Could not pause container",
            ContainerOp::Unpause => "Could not unpause container",
            ContainerOp::Kill => "Could not kill container",
        });
        let endpoint = self.endpoint_of(conn);
        self.operation(conn, failure, |ticket| DockerRequest::ContainerOp { ticket, endpoint, ids, op })
    }

    pub fn remove_objects(
        &mut self,
        conn: &str,
        object: DockerObject,
        ids: Vec<String>,
        force: bool,
        i18n: &mut I18n,
    ) -> Vec<DockerRequest> {
        if ids.is_empty() {
            return Vec::new();
        }
        let failure = i18n.tx(match object {
            DockerObject::Container => "Could not remove container",
            DockerObject::Image => "Could not remove image",
            DockerObject::Volume => "Could not remove volume",
            DockerObject::Network => "Could not remove network",
        });
        let endpoint = self.endpoint_of(conn);
        self.operation(conn, failure, |ticket| DockerRequest::Remove { ticket, endpoint, object, ids, force })
    }

    /// The answer to [`DockerRequest::ContainerOp`] or [`DockerRequest::Remove`]: a failure is
    /// reported, and the connection is read again either way.
    pub fn operation_done(&mut self, ticket: Ticket, result: Result<(), OpError>) -> Vec<DockerRequest> {
        let Some(op) = self.ops.remove(&ticket) else { return Vec::new() };
        let mut out = Vec::new();
        if let Err(OpError::Failed(error)) = result {
            out.push(notify(NotificationLevel::Error, op.failure, Some(error), "docker-op"));
        }
        out.extend(self.refresh_soon(&op.conn));
        out
    }

    /// Starts a long operation; only one runs at a time.
    fn begin_task(
        &mut self,
        conn: &str,
        title: String,
        failure: String,
        op: StreamOp,
        i18n: &mut I18n,
    ) -> Vec<DockerRequest> {
        if self.task.is_some() {
            let title = i18n.tx("Another Docker operation is still running");
            return vec![notify(NotificationLevel::Info, title, None, "docker-op")];
        }
        let ticket = self.ticket();
        self.task = Some(DockerTask { ticket, conn: conn.to_string(), title, failure, line: String::new() });
        vec![DockerRequest::Stream { ticket, endpoint: self.endpoint_of(conn), op }]
    }

    /// A line of output of the long operation.
    pub fn task_line(&mut self, ticket: Ticket, line: &str) {
        if let Some(task) = self.task.as_mut().filter(|task| task.ticket == ticket) {
            task.line = line.to_string();
        }
    }

    /// The long operation ended: a failure is reported, its last line is told, and the connection is
    /// read again.
    pub fn task_done(&mut self, ticket: Ticket, result: Result<(), OpError>) -> Vec<DockerRequest> {
        let Some(task) = self.task.take_if(|task| task.ticket == ticket) else { return Vec::new() };
        let mut out = Vec::new();
        if let Err(OpError::Failed(error)) = result {
            out.push(notify(NotificationLevel::Error, task.failure, Some(error), "docker-op"));
        }
        out.extend(self.refresh_soon(&task.conn));
        if !task.line.is_empty() {
            let title = task.title.strip_suffix('…').unwrap_or(&task.title).to_string();
            out.push(notify(NotificationLevel::Info, title, Some(task.line), "docker-task"));
        }
        out
    }

    /// Cancel on the long operation's banner.
    pub fn cancel_task(&self) -> Vec<DockerRequest> {
        self.task.iter().map(|task| DockerRequest::Cancel { ticket: task.ticket }).collect()
    }

    pub fn pull(&mut self, conn: &str, reference: &str, i18n: &mut I18n) -> Vec<DockerRequest> {
        let reference = reference.trim();
        if reference.is_empty() {
            return Vec::new();
        }
        let title = i18n.tx_built(&["Pulling ", "…"], &[Param::from(reference)]);
        let failure = i18n.tx_built(&["Could not pull ", ""], &[Param::from(reference)]);
        self.begin_task(conn, title, failure, StreamOp::Pull { reference: reference.to_string() }, i18n)
    }

    pub fn prune(&mut self, conn: &str, target: PruneTarget, i18n: &mut I18n) -> Vec<DockerRequest> {
        let title = i18n.tx(match target {
            PruneTarget::Containers => "Pruning stopped containers…",
            PruneTarget::Images => "Pruning dangling images…",
            PruneTarget::AllImages => "Pruning unused images…",
            PruneTarget::Volumes => "Pruning unused volumes…",
            PruneTarget::Networks => "Pruning unused networks…",
            PruneTarget::System => "Pruning the system…",
        });
        let failure = i18n.tx("Could not prune");
        self.begin_task(conn, title, failure, StreamOp::Prune(target), i18n)
    }

    /// A Compose operation on a project found through its containers' labels.
    pub fn compose(
        &mut self,
        conn: &str,
        project: &str,
        containers: &[DockerContainer],
        op: ComposeOp,
        i18n: &mut I18n,
    ) -> Vec<DockerRequest> {
        let (title, failure): ([&str; 2], [&str; 2]) = match op {
            ComposeOp::Start => (["Starting Compose project ", "…"], ["Could not start Compose project ", ""]),
            ComposeOp::Stop => (["Stopping Compose project ", "…"], ["Could not stop Compose project ", ""]),
            ComposeOp::Restart => (["Restarting Compose project ", "…"], ["Could not restart Compose project ", ""]),
            ComposeOp::Down => (["Taking down Compose project ", "…"], ["Could not take down Compose project ", ""]),
        };
        let name = [Param::from(project)];
        let (title, failure) = (i18n.tx_built(&title, &name), i18n.tx_built(&failure, &name));
        let first = containers.first();
        let op = StreamOp::Compose {
            project: project.to_string(),
            files: first.map(|container| container.compose_files.clone()).unwrap_or_default(),
            working_dir: first.and_then(|container| container.compose_working_dir.clone()),
            op,
        };
        self.begin_task(conn, title, failure, op, i18n)
    }

    /// The user agreed to `question`; `checked` is its checkbox.
    pub fn delete_confirmed(
        &mut self,
        question: &DeleteQuestion,
        checked: bool,
        i18n: &mut I18n,
    ) -> Vec<DockerRequest> {
        match &question.deletion {
            Deletion::Objects { object, ids, running } => {
                self.remove_objects(&question.conn, *object, ids.clone(), *running || checked, i18n)
            }
            Deletion::ComposeDown { project, files, working_dir } => {
                let op = StreamOp::Compose {
                    project: project.clone(),
                    files: files.clone(),
                    working_dir: working_dir.clone(),
                    op: ComposeOp::Down,
                };
                let name = [Param::from(project.as_str())];
                let title = i18n.tx_built(&["Taking down Compose project ", "…"], &name);
                let failure = i18n.tx_built(&["Could not take down Compose project ", ""], &name);
                self.begin_task(&question.conn, title, failure, op, i18n)
            }
        }
    }

    /// The question asked before deleting the node `target`.
    pub fn delete_question(target: &ConnectionNode, i18n: &mut I18n) -> Option<DeleteQuestion> {
        let label = Param::from(node_label(&target.node));
        let conn = target.conn.clone();
        let question = |title: &str, message: String, ok: &str, checkbox: Option<&str>, deletion: Deletion| {
            Some(DeleteQuestion {
                conn: conn.clone(),
                title: title.to_string(),
                message,
                ok: ok.to_string(),
                checkbox: checkbox.map(str::to_string),
                deletion,
            })
        };
        match &target.node {
            DockerNode::Compose { project, containers } => {
                let first = containers.first();
                let deletion = Deletion::ComposeDown {
                    project: project.clone(),
                    files: first.map(|container| container.compose_files.clone()).unwrap_or_default(),
                    working_dir: first.and_then(|container| container.compose_working_dir.clone()),
                };
                let message = i18n.tx_built(&["Stop and remove the containers and networks of '", "'?"], &[label]);
                question("Compose Down", message, "Down", None, deletion)
            }
            DockerNode::Container(container) => {
                let running = is_running(container);
                let message = if running {
                    i18n.tx_built(&["Delete container '", "'? It is running and will be stopped."], &[label])
                } else {
                    i18n.tx_built(&["Delete container '", "'?"], &[label])
                };
                let ids = vec![container.id.clone()];
                let deletion = Deletion::Objects { object: DockerObject::Container, ids, running };
                question("Delete Container", message, "Delete", None, deletion)
            }
            DockerNode::Image(image) => {
                let message = i18n.tx_built(&["Delete image '", "'?"], &[label]);
                let ids = vec![image.id.clone()];
                let deletion = Deletion::Objects { object: DockerObject::Image, ids, running: false };
                let force = "Force (also untag / remove when used by stopped containers)";
                question("Delete Image", message, "Delete", Some(force), deletion)
            }
            DockerNode::Volume(volume) => {
                let message = i18n.tx_built(&["Delete volume '", "'?"], &[label]);
                let ids = vec![volume.name.clone()];
                let deletion = Deletion::Objects { object: DockerObject::Volume, ids, running: false };
                question("Delete Volume", message, "Delete", None, deletion)
            }
            DockerNode::Network(network) => {
                let message = i18n.tx_built(&["Delete network '", "'?"], &[label]);
                let ids = vec![network.id.clone()];
                let deletion = Deletion::Objects { object: DockerObject::Network, ids, running: false };
                question("Delete Network", message, "Delete", None, deletion)
            }
            DockerNode::Docker | DockerNode::Group { .. } => None,
        }
    }

    /// Test Login on a registry: logs in through the first connection.
    pub fn test_login(&mut self, id: &str) -> Vec<DockerRequest> {
        let Some(registry) = self.registry(id).cloned() else { return Vec::new() };
        let endpoint = self.connections().first().map(|connection| connection.endpoint.clone()).unwrap_or_default();
        let ticket = self.ticket();
        self.logins.insert(ticket, registry.name.clone());
        vec![DockerRequest::RegistryLogin { ticket, endpoint, registry }]
    }

    /// The answer to [`DockerRequest::RegistryLogin`]: docker's answer, or why it failed.
    pub fn login_answered(
        &mut self,
        ticket: Ticket,
        answer: Result<String, String>,
        i18n: &mut I18n,
    ) -> Vec<DockerRequest> {
        let Some(name) = self.logins.remove(&ticket) else { return Vec::new() };
        match answer {
            Ok(answer) => vec![notify(NotificationLevel::Info, format!("{name}: {answer}"), None, "docker-login")],
            Err(error) => {
                let title = i18n.tx_built(&["Cannot log in to ", ""], &[Param::from(name)]);
                vec![notify(NotificationLevel::Error, title, Some(error), "docker-login")]
            }
        }
    }

    /// Reads the inspect payload of the node `key` for the detail pane; nothing for a node without
    /// one or whose payload is already there.
    pub fn request_inspect(&mut self, key: &str) -> Vec<DockerRequest> {
        if self.inspect.as_ref().is_some_and(|(_, state)| state.key == key) {
            return Vec::new();
        }
        let Some(target) = self.resolve(key) else { return Vec::new() };
        let Some((object, id)) = target.node.object() else { return Vec::new() };
        let (id, endpoint) = (id.to_string(), self.endpoint_of(&target.conn));
        let ticket = self.ticket();
        self.inspect = Some((ticket, InspectState { key: key.to_string(), result: None }));
        vec![DockerRequest::Inspect { ticket, endpoint, object, id }]
    }

    /// The answer to [`DockerRequest::Inspect`]; one for a node no longer shown is dropped.
    pub fn inspect_answered(&mut self, ticket: Ticket, answer: Result<String, String>) {
        if let Some((_, state)) = self.inspect.as_mut().filter(|(current, _)| *current == ticket) {
            state.result = Some(answer);
        }
    }

    /// The detail pane closed: an inspect payload in flight is no longer wanted.
    pub fn drop_inspect(&mut self) {
        self.inspect = None;
    }

    fn set_filters(&mut self, filters: DockerFilters) -> Vec<DockerRequest> {
        self.filters = filters;
        match serde_json::to_string(&filters) {
            Ok(text) => vec![DockerRequest::Store(StorageWrite { key: FILTERS_KEY.to_string(), value: Some(text) })],
            Err(error) => {
                tracing::warn!("services: cannot store the Docker filters: {error}");
                Vec::new()
            }
        }
    }

    // -----------------------------------------------------------------------------------------------
    // Commands

    /// Carries out what a Docker toolbar button or menu entry asks for, against the snapshots as they
    /// are now: a node that is gone or an operation that no longer applies does nothing.
    pub fn carry_out(&mut self, command: DockerCommand, i18n: &mut I18n) -> Vec<DockerRequest> {
        match command {
            DockerCommand::Refresh { conn } => self.refresh(&conn),
            DockerCommand::Start { key } => {
                let Some(target) = self.resolve(&key) else { return Vec::new() };
                if let DockerNode::Compose { project, containers } = &target.node {
                    return self.compose(&target.conn, project, containers, ComposeOp::Start, i18n);
                }
                let ids = node_containers(&target.node).into_iter().filter(|c| !is_running(c)).map(|c| c.id.clone());
                self.container_op(&target.conn, ids.collect(), ContainerOp::Start, i18n)
            }
            DockerCommand::Stop { key } => {
                let Some(target) = self.resolve(&key) else { return Vec::new() };
                if let DockerNode::Compose { project, containers } = &target.node {
                    return self.compose(&target.conn, project, containers, ComposeOp::Stop, i18n);
                }
                let ids = node_containers(&target.node).into_iter().filter(|c| is_running(c)).map(|c| c.id.clone());
                self.container_op(&target.conn, ids.collect(), ContainerOp::Stop, i18n)
            }
            DockerCommand::Restart { key } => {
                let Some(target) = self.resolve(&key) else { return Vec::new() };
                if let DockerNode::Compose { project, containers } = &target.node {
                    return self.compose(&target.conn, project, containers, ComposeOp::Restart, i18n);
                }
                let ids = node_containers(&target.node).into_iter().map(|c| c.id.clone()).collect();
                self.container_op(&target.conn, ids, ContainerOp::Restart, i18n)
            }
            DockerCommand::Pause { key } | DockerCommand::Unpause { key } => {
                let pause = self.resolve(&key);
                let Some(ConnectionNode { conn, node: DockerNode::Container(container) }) = pause else {
                    return Vec::new();
                };
                let op = if container.state == "paused" { ContainerOp::Unpause } else { ContainerOp::Pause };
                self.container_op(&conn, vec![container.id.clone()], op, i18n)
            }
            DockerCommand::Delete { key } => {
                let Some(target) = self.resolve(&key) else { return Vec::new() };
                Self::delete_question(&target, i18n)
                    .map(|question| DockerRequest::ConfirmDelete(Box::new(question)))
                    .into_iter()
                    .collect()
            }
            DockerCommand::Terminal { key } => {
                let Some(ConnectionNode { conn, node: DockerNode::Container(container) }) = self.resolve(&key) else {
                    return Vec::new();
                };
                if !is_running(&container) {
                    return Vec::new();
                }
                let snapshot = self.conns.get(&conn).and_then(|state| state.snapshot.as_ref());
                let executable = snapshot.and_then(|snapshot| snapshot.executable.clone());
                let program = executable.unwrap_or_else(|| "docker".to_string());
                let mut args = snapshot.map(|snapshot| snapshot.global_args.clone()).unwrap_or_default();
                let shell = "command -v bash >/dev/null 2>&1 && exec bash || exec sh";
                args.extend(["exec", "-it", container.id.as_str(), "sh", "-c", shell].map(str::to_string));
                vec![DockerRequest::OpenTerminal { program, args, name: container.name.clone() }]
            }
            DockerCommand::PullPrompt { conn } => vec![DockerRequest::PromptPull { conn }],
            DockerCommand::Pull { conn, reference } => self.pull(&conn, &reference, i18n),
            DockerCommand::PrunePrompt { conn, target } => {
                vec![DockerRequest::ConfirmPrune { conn, target, question: prune_question(target, i18n) }]
            }
            DockerCommand::ShowDetail(tab) => {
                self.detail_tab = tab;
                Vec::new()
            }
            DockerCommand::OpenLogInEditor { key } => match self.resolve(&key) {
                Some(ConnectionNode { conn, node: DockerNode::Container(container) }) => {
                    vec![DockerRequest::OpenLogInEditor { conn, container }]
                }
                _ => Vec::new(),
            },
            DockerCommand::Copy(text) => vec![DockerRequest::CopyText(text)],
            DockerCommand::ToggleStoppedContainers => {
                let filters = self.filters;
                self.set_filters(DockerFilters { show_stopped_containers: !filters.show_stopped_containers, ..filters })
            }
            DockerCommand::ToggleUntaggedImages => {
                let filters = self.filters;
                self.set_filters(DockerFilters { show_untagged_images: !filters.show_untagged_images, ..filters })
            }
            DockerCommand::AddConnection => vec![DockerRequest::ConnectionDialog(None)],
            DockerCommand::EditConnection(id) => {
                self.connection(&id).cloned().map(|c| DockerRequest::ConnectionDialog(Some(c))).into_iter().collect()
            }
            DockerCommand::RemoveConnection(id) => {
                let Some(connection) = self.connection(&id) else { return Vec::new() };
                let name = [Param::from(connection.name.as_str())];
                let message = i18n.tx_built(&["Remove the connection '", "' from Services?"], &name);
                let title = i18n.tx("Remove Docker Connection");
                vec![DockerRequest::ConfirmRemoveConnection { id, title, message }]
            }
            DockerCommand::AddRegistry => vec![DockerRequest::RegistryDialog(None)],
            DockerCommand::EditRegistry(id) => {
                self.registry(&id).cloned().map(|r| DockerRequest::RegistryDialog(Some(r))).into_iter().collect()
            }
            DockerCommand::TestLogin(id) => self.test_login(&id),
            DockerCommand::RemoveRegistry(id) => {
                let Some(registry) = self.registry(&id) else { return Vec::new() };
                let name = [Param::from(registry.name.as_str())];
                let message = i18n.tx_built(&["Remove the registry '", "' and its saved password?"], &name);
                let title = i18n.tx("Remove Docker Registry");
                vec![DockerRequest::ConfirmRemoveRegistry { id, title, message }]
            }
            DockerCommand::CancelTask => self.cancel_task(),
        }
    }

    /// Double click or Enter on a node: a container shows its log, a registry opens its dialog.
    pub fn activate(&mut self, key: &str) -> Vec<DockerRequest> {
        if let Some(registry) = parse_registry_key(key).and_then(|id| self.registry(id)) {
            return vec![DockerRequest::RegistryDialog(Some(registry.clone()))];
        }
        if let Some(ConnectionNode { node: DockerNode::Container(_), .. }) = self.resolve(key) {
            self.detail_tab = DetailTab::Log;
        }
        Vec::new()
    }

    // -----------------------------------------------------------------------------------------------
    // Toolbar and menus

    /// The toolbar buttons of the Docker node `key`.
    pub fn actions(&self, key: &str) -> Vec<ItemAction<NodeCommand>> {
        let Some(target) = self.resolve(key) else { return Vec::new() };
        let running_container = matches!(&target.node, DockerNode::Container(c) if is_running(c));
        let action = |id: &str, title: &str, icon: &str, enabled: bool, command: DockerCommand| ItemAction {
            id: id.to_string(),
            title: title.to_string(),
            icon: icon.to_string(),
            disabled: !enabled,
            command: docker(command),
        };
        let key = key.to_string();
        vec![
            action("refresh", "Refresh", "refresh", true, DockerCommand::Refresh { conn: target.conn.clone() }),
            action("start", "Start", "run", can_start(&target), DockerCommand::Start { key: key.clone() }),
            action("stop", "Stop", "stop", can_stop(&target), DockerCommand::Stop { key: key.clone() }),
            action("restart", "Restart", "rerun", can_restart(&target), DockerCommand::Restart { key: key.clone() }),
            action("delete", "Delete", "trash", can_remove(&target), DockerCommand::Delete { key: key.clone() }),
            action("terminal", "Create Terminal", "terminal", running_container, DockerCommand::Terminal { key }),
            action("pull", "Pull Image…", "docker-pull", true, DockerCommand::PullPrompt { conn: target.conn }),
        ]
    }

    /// View Options: the filters, each a toggle.
    pub fn view_options_menu(&self) -> Vec<ServiceMenuEntry<NodeCommand>> {
        vec![
            ServiceMenuEntry::row("Stopped Containers", None, docker(DockerCommand::ToggleStoppedContainers))
                .checked(self.filters.show_stopped_containers),
            ServiceMenuEntry::row("Untagged Images", None, docker(DockerCommand::ToggleUntaggedImages))
                .checked(self.filters.show_untagged_images),
        ]
    }

    /// The provider's "Add Service" entries.
    pub fn add_menu(&self) -> Vec<ServiceMenuEntry<NodeCommand>> {
        vec![
            ServiceMenuEntry::row("Docker Registry…", Some("docker-registry"), docker(DockerCommand::AddRegistry)),
            ServiceMenuEntry::row("Docker Connection…", Some("file-docker"), docker(DockerCommand::AddConnection)),
        ]
    }

    /// The context menu of the Docker node `key`.
    pub fn menu(&self, key: &str) -> Vec<ServiceMenuEntry<NodeCommand>> {
        type Entry = ServiceMenuEntry<NodeCommand>;
        let row =
            |label: &str, icon: Option<&'static str>, command: DockerCommand| Entry::row(label, icon, docker(command));
        if let Some(registry) = parse_registry_key(key).and_then(|id| self.registry(id)) {
            let id = registry.id.clone();
            return vec![
                row("Edit Registry…", Some("edit"), DockerCommand::EditRegistry(id.clone())),
                row("Test Login", None, DockerCommand::TestLogin(id.clone())),
                Entry::Separator,
                row("Remove Registry", Some("trash"), DockerCommand::RemoveRegistry(id)).danger(),
            ];
        }
        let Some(target) = self.resolve(key) else { return Vec::new() };
        let (conn, key) = (target.conn.clone(), key.to_string());
        let mut items: Vec<Entry> = Vec::new();
        if matches!(target.node, DockerNode::Container(_) | DockerNode::Compose { .. }) {
            let (start, stop) = (!can_start(&target), !can_stop(&target));
            items.push(row("Start", Some("run"), DockerCommand::Start { key: key.clone() }).disabled(start));
            items.push(row("Stop", Some("stop"), DockerCommand::Stop { key: key.clone() }).disabled(stop));
            items.push(row("Restart", Some("rerun"), DockerCommand::Restart { key: key.clone() }));
        }
        match &target.node {
            DockerNode::Container(container) => {
                let running = is_running(container);
                items.push(if container.state == "paused" {
                    row("Unpause", Some("resume"), DockerCommand::Unpause { key: key.clone() })
                } else {
                    row("Pause", Some("pause"), DockerCommand::Pause { key: key.clone() }).disabled(!running)
                });
                let log_in_editor = DockerCommand::OpenLogInEditor { key: key.clone() };
                let processes = DockerCommand::ShowDetail(DetailTab::Processes);
                let terminal = DockerCommand::Terminal { key: key.clone() };
                items.extend([
                    Entry::Separator,
                    row("Dashboard", Some("layout"), DockerCommand::ShowDetail(DetailTab::Dashboard)),
                    row("Show Log", Some("file-log"), DockerCommand::ShowDetail(DetailTab::Log)),
                    row("Open Log in Editor", Some("external-link"), log_in_editor),
                    row("Show Processes", Some("list"), processes).disabled(!running),
                    row("Inspect", Some("info"), DockerCommand::ShowDetail(DetailTab::Inspect)),
                    row("Create Terminal", Some("terminal"), terminal).disabled(!running),
                    Entry::Separator,
                    row("Copy Container ID", Some("copy"), DockerCommand::Copy(container.id.clone())),
                    row("Copy Image Name", None, DockerCommand::Copy(container.image.clone())),
                ]);
            }
            DockerNode::Image(image) => {
                items.push(row("Dashboard", Some("layout"), DockerCommand::ShowDetail(DetailTab::Dashboard)));
                items.push(row("Show Layers", Some("list"), DockerCommand::ShowDetail(DetailTab::Layers)));
                items.push(row("Inspect", Some("info"), DockerCommand::ShowDetail(DetailTab::Inspect)));
                let pull = DockerCommand::Pull { conn: conn.clone(), reference: image.reference() };
                items.push(row("Pull Again", Some("docker-pull"), pull).disabled(image.repository == "<none>"));
                items.push(row("Copy Image ID", Some("copy"), DockerCommand::Copy(short_id(&image.id).to_string())));
            }
            DockerNode::Volume(_) | DockerNode::Network(_) => {
                items.push(row("Dashboard", Some("layout"), DockerCommand::ShowDetail(DetailTab::Dashboard)));
                items.push(row("Inspect", Some("info"), DockerCommand::ShowDetail(DetailTab::Inspect)));
            }
            DockerNode::Group { group, .. } => {
                if *group == GroupKind::Images {
                    let pull = DockerCommand::PullPrompt { conn: conn.clone() };
                    items.push(row("Pull Image…", Some("docker-pull"), pull));
                }
                match group {
                    GroupKind::Containers | GroupKind::DevContainers => items.push(
                        row("Stopped Containers", None, DockerCommand::ToggleStoppedContainers)
                            .checked(self.filters.show_stopped_containers),
                    ),
                    GroupKind::Images => items.push(
                        row("Untagged Images", None, DockerCommand::ToggleUntaggedImages)
                            .checked(self.filters.show_untagged_images),
                    ),
                    GroupKind::Volumes | GroupKind::Networks => {}
                }
                let prune_target = match group {
                    GroupKind::Containers | GroupKind::DevContainers => PruneTarget::Containers,
                    GroupKind::Images => PruneTarget::Images,
                    GroupKind::Volumes => PruneTarget::Volumes,
                    GroupKind::Networks => PruneTarget::Networks,
                };
                let prune = DockerCommand::PrunePrompt { conn: conn.clone(), target: prune_target };
                items.push(row(prune_label(prune_target), None, prune));
            }
            DockerNode::Docker => {
                let prune = PRUNE.iter().map(|(target, label, _)| {
                    row(label, None, DockerCommand::PrunePrompt { conn: conn.clone(), target: *target })
                });
                items.extend([
                    row("Refresh", Some("refresh"), DockerCommand::Refresh { conn: conn.clone() }),
                    row("Pull Image…", Some("docker-pull"), DockerCommand::PullPrompt { conn: conn.clone() }),
                    Entry::submenu("View Options", Some("filter"), self.view_options_menu()),
                    Entry::submenu("Prune", None, prune.collect()),
                ]);
            }
            DockerNode::Compose { .. } => {}
        }
        if can_remove(&target) {
            let label = if matches!(target.node, DockerNode::Compose { .. }) { "Compose Down" } else { "Delete…" };
            items.extend([Entry::Separator, row(label, Some("trash"), DockerCommand::Delete { key }).danger()]);
        }
        if let (DockerNode::Docker, Some(connection)) = (&target.node, self.connection(&conn)) {
            let id = connection.id.clone();
            items.extend([
                Entry::Separator,
                row("Edit Connection…", Some("edit"), DockerCommand::EditConnection(id.clone())),
                row("Remove Connection", Some("trash"), DockerCommand::RemoveConnection(id)).danger(),
            ]);
        }
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::model::expandable_keys;

    fn container(name: &str, state: &str, project: Option<&str>, service: Option<&str>) -> DockerContainer {
        DockerContainer {
            id: format!("{name}-id"),
            name: name.to_string(),
            image: "img".to_string(),
            command: String::new(),
            state: state.to_string(),
            status: if state == "running" { "Up 1 hour" } else { "Exited (0)" }.to_string(),
            ports: String::new(),
            created_at: String::new(),
            compose_project: project.map(str::to_string),
            compose_service: service.map(str::to_string),
            compose_working_dir: None,
            compose_files: Vec::new(),
            mounts: Vec::new(),
            networks: Vec::new(),
            labels: BTreeMap::new(),
        }
    }

    fn image(id: &str, repository: &str, tag: &str) -> DockerImage {
        DockerImage {
            id: id.to_string(),
            repository: repository.to_string(),
            tag: tag.to_string(),
            size: "1MB".to_string(),
            created_since: "now".to_string(),
        }
    }

    fn ready() -> DockerAvailability {
        DockerAvailability::Ready { client_version: "27".into(), server_version: "27".into(), context: None }
    }

    fn snapshot() -> DockerSnapshot {
        DockerSnapshot {
            availability: ready(),
            executable: Some("/usr/bin/docker".to_string()),
            containers: vec![
                container("web", "exited", None, None),
                container("shop-db-1", "running", Some("shop"), Some("db")),
                container("shop-api-1", "exited", Some("shop"), Some("api")),
            ],
            images: vec![image("sha256:abc", "postgres", "16")],
            volumes: Vec::new(),
            networks: Vec::new(),
            errors: Vec::new(),
            global_args: Vec::new(),
        }
    }

    fn labels(nodes: &[DockerNode]) -> Vec<String> {
        nodes.iter().map(node_label).collect()
    }

    fn icons(nodes: &[DockerNode]) -> Vec<&'static str> {
        nodes.iter().map(node_icon).collect()
    }

    fn children(snapshot: &DockerSnapshot, node: &DockerNode, filters: DockerFilters) -> Vec<DockerNode> {
        children_of(Some(snapshot), node, filters).expect("the node has children")
    }

    // services tree

    #[test]
    fn groups_compose_projects_at_the_top_followed_by_dev_containers_containers_images_networks_and_volumes() {
        let snap = snapshot();
        let all = DockerFilters::default();
        let groups = children(&snap, &root_node(), all);
        assert_eq!(
            labels(&groups),
            ["Docker-compose: shop", "Dev Containers", "Containers", "Images", "Networks", "Volumes"]
        );
        assert_eq!(
            icons(&groups),
            [
                "docker-compose-running",
                "docker-devcontainer",
                "docker-containers",
                "docker-images",
                "docker-networks",
                "docker-volumes"
            ]
        );
        assert_eq!(node_detail(&groups[0]).as_deref(), Some("1/2 running"));
        let shop = children(&snap, &groups[0], all);
        assert_eq!(labels(&shop), ["api", "db"]);
        assert_eq!(icons(&shop), ["docker-compose-service-stopped", "docker-compose-service-running"]);
        assert_eq!(node_detail(&groups[2]).as_deref(), Some("1"));
        let containers = children(&snap, &groups[2], all);
        assert!(matches!(containers.as_slice(), [DockerNode::Container(_)]));
        assert_eq!(node_icon(&containers[0]), "docker-container-stopped");
        let images = children(&snap, &groups[3], all);
        assert_eq!(node_label(&images[0]), "postgres:16");
        assert_eq!(node_icon(&images[0]), "docker-image");
    }

    #[test]
    fn finds_nodes_again_in_a_new_snapshot() {
        let snap = snapshot();
        assert!(matches!(find_node(Some(&snap), "container:shop-db-1-id"), Some(DockerNode::Container(_))));
        assert_eq!(find_node(Some(&snap), "container:gone"), None);
    }

    #[test]
    fn has_no_children_while_docker_is_down() {
        let down = DockerSnapshot {
            availability: DockerAvailability::DaemonDown { message: "x".into(), context: None },
            ..snapshot()
        };
        assert_eq!(children_of(Some(&down), &root_node(), DockerFilters::default()), None);
    }

    // services tree host

    fn connections() -> Vec<DockerConnection> {
        let tcp = DockerEndpoint::Tcp { host: "build:2376".into(), tls_cert_dir: None };
        vec![
            DockerConnection { id: "default".into(), name: "Docker".into(), endpoint: DockerEndpoint::Default },
            DockerConnection { id: "c2".into(), name: "Build box".into(), endpoint: tcp },
        ]
    }

    fn states() -> BTreeMap<String, ConnectionState> {
        let state = ConnectionState { snapshot: Some(snapshot()), loading: false, error: None };
        BTreeMap::from([("default".to_string(), state)])
    }

    #[test]
    fn puts_every_connection_and_registry_at_the_root_with_connection_scoped_keys() {
        let ghcr =
            DockerRegistry { id: "r1".into(), name: "GHCR".into(), address: "ghcr.io".into(), username: "me".into() };
        let items = docker_items(&connections(), &[ghcr], &states(), DockerFilters::default());
        let shown: Vec<(&str, &str, Option<&str>, &str)> = items
            .iter()
            .map(|item| (item.key.as_str(), item.label.as_str(), item.detail.as_deref(), item.icon.as_str()))
            .collect();
        assert_eq!(
            shown,
            [
                ("docker:default", "Docker", None, "docker-connected"),
                ("docker:c2", "Build box", None, "file-docker"),
                ("docker-registry:r1", "GHCR", Some("ghcr.io"), "docker-registry"),
            ]
        );
        assert!(items[1].dim);
        assert_eq!(items[1].children, Some(Vec::new()));
        let top = items[0].children.as_deref().expect("the connection has children");
        assert_eq!(top[0].key, "docker:default:compose:shop");
        let containers = &top[2];
        assert_eq!(containers.key, "docker:default:group:containers");
        let keys: Vec<&str> = containers.children.iter().flatten().map(|child| child.key.as_str()).collect();
        assert_eq!(keys, ["docker:default:container:web-id"]);
        assert!(containers.children.as_deref().expect("listed")[0].dim);
        let mut tree = crate::services::model::ServicesTree::new();
        tree.set_provider_roots(DOCKER_PROVIDER, 10, items.clone(), false);
        let db = tree.locate("docker:default:container:shop-db-1-id").expect("indexed");
        assert_eq!(db.node.label, "db");
        assert!(expandable_keys(&items).contains(&"docker:default:compose:shop".to_string()));
    }

    #[test]
    fn round_trips_tree_keys() {
        let snap = snapshot();
        let node = find_node(Some(&snap), "container:web-id").expect("found");
        assert_eq!(tree_key("c2", &node), "docker:c2:container:web-id");
        assert_eq!(parse_tree_key("docker:c2:container:web-id"), Some(("c2", "container:web-id")));
        assert_eq!(parse_tree_key("docker:c2"), Some(("c2", "docker")));
        assert_eq!(tree_key("c2", &root_node()), "docker:c2");
        assert_eq!(parse_tree_key("run:type:cargo"), None);
    }

    // filters, cross-references and dashboard parsers

    fn rich_snapshot() -> DockerSnapshot {
        let with = |mut c: DockerContainer, image: &str, ports: &str, mounts: &[&str], networks: &[&str]| {
            c.image = image.to_string();
            c.ports = ports.to_string();
            c.mounts = mounts.iter().map(|m| m.to_string()).collect();
            c.networks = networks.iter().map(|n| n.to_string()).collect();
            c
        };
        let sized = |mut i: DockerImage, size: &str, since: &str| {
            i.size = size.to_string();
            i.created_since = since.to_string();
            i
        };
        DockerSnapshot {
            containers: vec![
                with(container("web", "exited", None, None), "nginx:1.27", "0.0.0.0:8080->80/tcp", &[], &["bridge"]),
                with(
                    container("shop-db-1", "running", Some("shop"), Some("db")),
                    "postgres:16",
                    "127.0.0.1:5432->5432/tcp",
                    &["shop_pgdata"],
                    &["shop_default"],
                ),
                with(
                    container("shop-api-1", "exited", Some("shop"), Some("api")),
                    "postgres:16",
                    "",
                    &[],
                    &["shop_default"],
                ),
            ],
            images: vec![
                sized(image("sha256:1111111111112222", "postgres", "16"), "431MB", "7 weeks ago"),
                sized(image("sha256:1111111111112222", "postgres", "latest"), "431MB", "7 weeks ago"),
                sized(image("sha256:aaaaaaaaaaaa0000", "<none>", "<none>"), "120MB", "4 weeks ago"),
            ],
            volumes: vec![DockerVolume {
                name: "shop_pgdata".into(),
                driver: "local".into(),
                mountpoint: "/var/lib/docker/volumes/shop_pgdata/_data".into(),
                compose_project: Some("shop".into()),
                scope: String::new(),
                labels: BTreeMap::new(),
            }],
            networks: vec![DockerNetwork {
                id: "net-1".into(),
                name: "shop_default".into(),
                driver: "bridge".into(),
                scope: "local".into(),
                compose_project: Some("shop".into()),
                created_at: String::new(),
                ipv6: false,
                internal: false,
                labels: BTreeMap::new(),
            }],
            ..snapshot()
        }
    }

    #[test]
    fn filters_stopped_containers_and_untagged_images_when_toggled_off() {
        let snap = rich_snapshot();
        let none = DockerFilters { show_stopped_containers: false, show_untagged_images: false };
        let groups = children(&snap, &root_node(), none);
        assert_eq!(
            labels(&groups),
            ["Docker-compose: shop", "Dev Containers", "Containers", "Images", "Networks", "Volumes"]
        );
        assert_eq!(labels(&children(&snap, &groups[0], none)), ["db"]);
        assert_eq!(node_detail(&groups[2]), None);
        assert_eq!(node_detail(&groups[3]).as_deref(), Some("2"));
        assert_eq!(labels(&children(&snap, &groups[3], none)), ["postgres:16", "postgres:latest"]);
        assert!(is_untagged_image(&snap.images[2]));
        assert!(!is_untagged_image(&snap.images[0]));

        let mut store = DockerStore::new(None);
        let mut i18n = I18n::default();
        let written = store.carry_out(DockerCommand::ToggleStoppedContainers, &mut i18n);
        assert!(!store.filters().show_stopped_containers);
        let Some(DockerRequest::Store(write)) = written.first() else { panic!("the filters are stored") };
        assert_eq!(write.key, FILTERS_KEY);
        assert_eq!(DockerStore::new(write.value.as_deref()).filters(), store.filters());
        store.carry_out(DockerCommand::ToggleStoppedContainers, &mut i18n);
        assert_eq!(store.filters(), DockerFilters::default());
    }

    #[test]
    fn cross_references_images_volumes_and_networks_with_containers() {
        let snap = rich_snapshot();
        let names = |containers: Vec<&DockerContainer>| -> Vec<String> {
            containers.into_iter().map(|container| container.name.clone()).collect()
        };
        let pg = &snap.images[0];
        assert_eq!(names(containers_using_image(&snap, pg)), ["shop-db-1", "shop-api-1"]);
        assert_eq!(image_tags_for_id(&snap, pg), ["postgres:16", "postgres:latest"]);
        let runs = find_image_for_container(&snap, &snap.containers[1]).map(|image| image.id.as_str());
        assert_eq!(runs, Some(pg.id.as_str()));
        assert_eq!(names(containers_using_volume(&snap, &snap.volumes[0])), ["shop-db-1"]);
        assert_eq!(names(containers_in_network(&snap, &snap.networks[0])), ["shop-db-1", "shop-api-1"]);
    }

    // the store's flows

    fn loaded_store() -> (DockerStore, Vec<DockerRequest>) {
        let mut store = DockerStore::new(None);
        let requests = store.watch();
        let [DockerRequest::LoadSettings { ticket }] = requests.as_slice() else { panic!("the settings load first") };
        let settings = DockerSettings { connections: connections(), registries: Vec::new() };
        let requests = store.settings_loaded(*ticket, Ok(settings));
        (store, requests)
    }

    fn snapshot_tickets(requests: &[DockerRequest]) -> Vec<(String, Ticket)> {
        requests
            .iter()
            .filter_map(|request| match request {
                DockerRequest::Snapshot { ticket, conn, .. } => Some((conn.clone(), *ticket)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn reads_every_connection_once_and_coalesces_refreshes_into_one_follow_up() {
        let (mut store, requests) = loaded_store();
        let reads = snapshot_tickets(&requests);
        assert_eq!(reads.iter().map(|(conn, _)| conn.as_str()).collect::<Vec<_>>(), ["default", "c2"]);
        assert!(store.refresh("default").is_empty());
        assert!(store.refresh("default").is_empty());
        let (_, ticket) = reads[0].clone();
        let answered = store.snapshot_answered(ticket, Ok(snapshot()));
        assert!(matches!(answered.as_slice(), [DockerRequest::WatchEvents { .. }, DockerRequest::Snapshot { .. }]));
        assert!(store.connection_state("default").is_some_and(ConnectionState::is_ready));
        assert!(store.snapshot_answered(ticket, Ok(snapshot())).is_empty(), "an old answer is dropped");
        assert_eq!(store.roots()[0].icon, "docker-connected");
    }

    #[test]
    fn follows_events_while_watched_and_polls_a_daemon_that_is_down() {
        let (mut store, requests) = loaded_store();
        let reads = snapshot_tickets(&requests);
        let started = store.snapshot_answered(reads[0].1, Ok(snapshot()));
        let Some(DockerRequest::WatchEvents { ticket: events, .. }) = started.first() else { panic!("events start") };
        let down = DockerSnapshot {
            availability: DockerAvailability::DaemonDown { message: "x".into(), context: None },
            ..snapshot()
        };
        store.snapshot_answered(reads[1].1, Ok(down));
        assert_eq!(store.roots()[1].detail.as_deref(), Some("not running"));
        let polled = snapshot_tickets(&store.poll());
        assert_eq!(polled.iter().map(|(conn, _)| conn.as_str()).collect::<Vec<_>>(), ["c2"]);
        let soon = store.event(*events);
        let [DockerRequest::RefreshLater { ticket: first, .. }] = soon.as_slice() else { panic!("a debounce") };
        let again = store.event(*events);
        let [DockerRequest::RefreshLater { ticket: second, .. }] = again.as_slice() else { panic!("a debounce") };
        assert!(store.refresh_due(*first).is_empty(), "a replaced wait does nothing");
        assert_eq!(snapshot_tickets(&store.refresh_due(*second)).len(), 1);
        assert_eq!(store.unwatch(), [DockerRequest::Cancel { ticket: *events }]);
        assert!(store.event(*events).is_empty());
    }

    #[test]
    fn runs_one_long_operation_at_a_time_and_reports_its_end() {
        let (mut store, _) = loaded_store();
        let mut i18n = I18n::default();
        let pulled = store.pull("default", "postgres:16", &mut i18n);
        let [DockerRequest::Stream { ticket, op: StreamOp::Pull { reference }, .. }] = pulled.as_slice() else {
            panic!("the pull streams")
        };
        assert_eq!(reference, "postgres:16");
        assert_eq!(store.task().map(|task| task.title.as_str()), Some("Pulling postgres:16…"));
        let busy = store.prune("default", PruneTarget::System, &mut i18n);
        assert!(matches!(busy.as_slice(), [DockerRequest::Notify(_)]));
        store.task_line(*ticket, "Status: Downloaded newer image");
        assert_eq!(store.cancel_task(), [DockerRequest::Cancel { ticket: *ticket }]);
        let done = store.task_done(*ticket, Err(OpError::Cancelled));
        assert!(store.task().is_none());
        let Some(DockerRequest::Notify(last)) = done.last() else { panic!("the last line is told") };
        let told = (last.title.as_str(), last.body.as_deref());
        assert_eq!(told, ("Pulling postgres:16", Some("Status: Downloaded newer image")));
        assert_eq!(done.iter().filter(|request| matches!(request, DockerRequest::Notify(_))).count(), 1);
    }

    #[test]
    fn asks_before_deleting_and_reports_a_failed_operation() {
        let (mut store, requests) = loaded_store();
        let reads = snapshot_tickets(&requests);
        store.snapshot_answered(reads[0].1, Ok(snapshot()));
        let mut i18n = I18n::default();
        let key = "docker:default:container:shop-db-1-id";
        let asked = store.carry_out(DockerCommand::Delete { key: key.to_string() }, &mut i18n);
        let [DockerRequest::ConfirmDelete(question)] = asked.as_slice() else { panic!("the delete is confirmed") };
        assert_eq!(question.message, "Delete container 'db'? It is running and will be stopped.");
        let removed = store.delete_confirmed(question, false, &mut i18n);
        let [DockerRequest::Remove { ticket, force, ids, .. }] = removed.as_slice() else { panic!("removes") };
        assert!(*force && ids == &["shop-db-1-id".to_string()]);
        let done = store.operation_done(*ticket, Err(OpError::Failed("conflict".into())));
        let Some(DockerRequest::Notify(failure)) = done.first() else { panic!("the failure is reported") };
        assert_eq!(failure.title, "Could not remove container");
        assert!(matches!(done.last(), Some(DockerRequest::RefreshLater { .. })));
        let compose = store.carry_out(DockerCommand::Start { key: "docker:default:compose:shop".into() }, &mut i18n);
        let [DockerRequest::Stream { op: StreamOp::Compose { op, project, .. }, .. }] = compose.as_slice() else {
            panic!("a Compose project starts through Compose")
        };
        assert_eq!((*op, project.as_str()), (ComposeOp::Start, "shop"));
    }

    #[test]
    fn offers_the_actions_and_menus_of_containers_groups_and_connections() {
        let (mut store, requests) = loaded_store();
        let reads = snapshot_tickets(&requests);
        store.snapshot_answered(reads[0].1, Ok(snapshot()));
        let enabled = |actions: Vec<ItemAction<NodeCommand>>| -> Vec<String> {
            actions.into_iter().filter(|action| !action.disabled).map(|action| action.id).collect()
        };
        let db = "docker:default:container:shop-db-1-id";
        assert_eq!(enabled(store.actions(db)), ["refresh", "stop", "restart", "delete", "terminal", "pull"]);
        assert_eq!(enabled(store.actions("docker:default:group:images")), ["refresh", "pull"]);
        let labels = |entries: Vec<ServiceMenuEntry<NodeCommand>>| -> Vec<String> {
            entries
                .into_iter()
                .map(|entry| match entry {
                    ServiceMenuEntry::Separator => "-".to_string(),
                    ServiceMenuEntry::Row(row) if row.checked => format!("[x] {}", row.label),
                    ServiceMenuEntry::Row(row) => row.label,
                })
                .collect()
        };
        assert_eq!(
            labels(store.menu("docker:default:group:containers")),
            ["[x] Stopped Containers", "Prune Unused Containers…"]
        );
        let connection = labels(store.menu("docker:default"));
        assert_eq!(connection.last().map(String::as_str), Some("Remove Connection"));
        assert!(connection.contains(&"View Options".to_string()));
        assert!(store.actions("run:type:npm").is_empty());
        let mut i18n = I18n::default();
        let terminal = store.carry_out(DockerCommand::Terminal { key: db.to_string() }, &mut i18n);
        let [DockerRequest::OpenTerminal { program, args, name }] = terminal.as_slice() else { panic!("a terminal") };
        assert_eq!((program.as_str(), name.as_str()), ("/usr/bin/docker", "shop-db-1"));
        assert_eq!(&args[..3], ["exec", "-it", "shop-db-1-id"]);
    }

    #[test]
    fn saves_connections_after_the_settings_load_and_applies_them_once_saved() {
        let mut store = DockerStore::new(None);
        let added = DockerConnection { id: "c3".into(), name: "Lab".into(), endpoint: DockerEndpoint::Default };
        let requests = store.save_connection(added.clone());
        let [DockerRequest::LoadSettings { ticket }] = requests.as_slice() else { panic!("loads first") };
        let saved = store.settings_loaded(*ticket, Ok(DockerSettings::default()));
        let [DockerRequest::SaveSettings { ticket, settings }] = saved.as_slice() else { panic!("saves") };
        assert_eq!(settings.connections.len(), 2);
        assert!(store.connections().iter().all(|connection| connection.id != "c3"), "not before it is saved");
        store.settings_saved(*ticket, Ok(()));
        assert!(store.connections().contains(&added));
        let removed = store.remove_connection("c3");
        let Some(DockerRequest::SaveSettings { ticket, .. }) = removed.last() else { panic!("saves") };
        let failed = store.settings_saved(*ticket, Err("read-only".into()));
        assert!(matches!(failed.as_slice(), [DockerRequest::Notify(_)]));
        assert!(store.connections().contains(&added));
    }
}
