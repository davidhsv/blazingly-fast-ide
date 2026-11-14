//! The Docker tree of the Services window drawn from snapshots the test answers itself: the store's
//! reads are taken from it before the window carries them out, so no Docker program runs and no
//! daemon is reached.

use super::*;
use crate::services::docker::{DockerRequest, Ticket};
use ide_docker::{DockerAvailability, DockerConnection, DockerContainer, DockerEndpoint, DockerImage, DockerSnapshot};
use std::collections::BTreeMap;

const CONN: &str = "fake";

/// What the Services window draws its tree from.
type State = Entity<super::super::super::services_window::ServicesState>;

fn container(name: &str, state: &str) -> DockerContainer {
    DockerContainer {
        id: format!("{name}-id"),
        name: name.to_owned(),
        image: "img".to_owned(),
        command: String::new(),
        state: state.to_owned(),
        status: if state == "running" { "Up 1 hour" } else { "Exited (0)" }.to_owned(),
        ports: String::new(),
        created_at: String::new(),
        compose_project: None,
        compose_service: None,
        compose_working_dir: None,
        compose_files: Vec::new(),
        mounts: Vec::new(),
        networks: Vec::new(),
        labels: BTreeMap::new(),
    }
}

fn image(id: &str, repository: &str, tag: &str) -> DockerImage {
    DockerImage {
        id: id.to_owned(),
        repository: repository.to_owned(),
        tag: tag.to_owned(),
        size: "1MB".to_owned(),
        created_since: "now".to_owned(),
    }
}

/// A ready daemon with `containers` and a tagged and an untagged image.
fn snapshot(containers: Vec<DockerContainer>) -> DockerSnapshot {
    DockerSnapshot {
        availability: DockerAvailability::Ready {
            client_version: "27".into(),
            server_version: "27".into(),
            context: None,
        },
        executable: Some("/usr/bin/docker".to_owned()),
        containers,
        images: vec![image("sha256:pg", "postgres", "16"), image("sha256:dangling", "<none>", "<none>")],
        volumes: Vec::new(),
        networks: Vec::new(),
        errors: Vec::new(),
        global_args: Vec::new(),
    }
}

/// The snapshot reads among `requests`.
fn snapshot_reads(requests: &[DockerRequest]) -> Vec<Ticket> {
    requests
        .iter()
        .filter_map(|request| match request {
            DockerRequest::Snapshot { ticket, .. } => Some(*ticket),
            _ => None,
        })
        .collect()
}

/// The Services window shown with its Docker settings read, and its state.
fn shown_services(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> State {
    run_action(cx, "services.show", None);
    let state = workbench
        .read_with(cx, |w, _| w.services.as_ref().map(|services| services.state().clone()))
        .expect("the Services window is built");
    wait_long(cx, "the Docker settings are read", |cx| {
        state.update(cx, |state, _| state.docker_mut().settings().is_some())
    });
    state
}

/// Adds a connection to the store as saved, without writing or reading anything: the read it asks for is
/// returned, not carried out. Its socket is in a temporary folder no daemon listens on.
fn add_connection(state: &State, socket: &std::path::Path, cx: &mut VisualTestContext) -> Ticket {
    let connection = DockerConnection {
        id: CONN.to_owned(),
        name: "Fake Docker".to_owned(),
        endpoint: DockerEndpoint::Socket { path: socket.to_string_lossy().into_owned() },
    };
    let reads = state.update(cx, |state, cx| {
        cx.notify();
        let docker = state.docker_mut();
        let saving = docker.save_connection(connection);
        let saved = saving.iter().find_map(|request| match request {
            DockerRequest::SaveSettings { ticket, .. } => Some(*ticket),
            _ => None,
        });
        docker.settings_saved(saved.expect("the connection is saved"), Ok(()))
    });
    cx.run_until_parked();
    let reads = snapshot_reads(&reads);
    assert_eq!(reads.len(), 1, "a watched tree reads the new connection");
    reads[0]
}

/// Answers the read `ticket` with `answer`; the reads that follow, the events stream left aside.
fn answer(state: &State, ticket: Ticket, answer: DockerSnapshot, cx: &mut VisualTestContext) -> Vec<Ticket> {
    let more = state.update(cx, |state, cx| {
        cx.notify();
        state.docker_mut().snapshot_answered(ticket, Ok(answer))
    });
    cx.run_until_parked();
    snapshot_reads(&more)
}

/// Every Docker row of the tree, opened all the way down: the keys local to the connection.
fn docker_rows(view: &Entity<ServicesView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.update(cx, |view, cx| view.expand_all(cx));
    cx.run_until_parked();
    let prefix = format!("docker:{CONN}:");
    let rows = view.read_with(cx, |view, _| view.rows());
    rows.into_iter().filter_map(|row| row.key.strip_prefix(&prefix).map(str::to_owned)).collect()
}

fn shows(view: &Entity<ServicesView>, key: &str, cx: &mut VisualTestContext) -> bool {
    docker_rows(view, cx).iter().any(|row| row == key)
}

/// Refresh Docker reads the connection again: run while a read is under way, a second read follows it, and
/// the tree shows what that one found.
#[gpui_kit::test]
async fn refresh_docker_reads_the_connection_again_and_the_tree_shows_what_it_found(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = services_project(cx, Vec::new());
    let state = shown_services(&workbench, cx);
    let view = services_view(&workbench, cx);
    let sockets = tempfile::tempdir().expect("a temporary folder");
    let first = add_connection(&state, &sockets.path().join("none.sock"), cx);

    run_action(cx, "docker.refresh", None);
    let follows = answer(&state, first, snapshot(vec![container("web", "running")]), cx);
    assert_eq!(follows.len(), 1, "the refresh asked meanwhile reads the connection again");
    wait_long(cx, "the first snapshot shows", |cx| shows(&view, "container:web-id", cx));

    answer(&state, follows[0], snapshot(vec![container("web", "running"), container("fresh", "running")]), cx);
    wait_long(cx, "the second snapshot shows", |cx| shows(&view, "container:fresh-id", cx));
    close_project(cx);
}

/// Show Stopped Containers, run as an action, takes the stopped container out of the Docker tree and, run
/// again, puts it back; the running one stays throughout.
#[gpui_kit::test]
async fn show_stopped_containers_leaves_the_stopped_container_out_of_the_tree(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = services_project(cx, Vec::new());
    let state = shown_services(&workbench, cx);
    let view = services_view(&workbench, cx);
    let sockets = tempfile::tempdir().expect("a temporary folder");
    let first = add_connection(&state, &sockets.path().join("none.sock"), cx);
    answer(&state, first, snapshot(vec![container("api", "running"), container("worker", "exited")]), cx);
    wait_long(cx, "both containers show", |cx| {
        shows(&view, "container:api-id", cx) && shows(&view, "container:worker-id", cx)
    });

    run_action(cx, "docker.toggleStoppedContainers", None);
    wait_long(cx, "the stopped container goes", |cx| !shows(&view, "container:worker-id", cx));
    assert!(shows(&view, "container:api-id", cx), "the running container stays");
    assert!(shows(&view, "image:sha256:dangling", cx), "the images are left alone");

    run_action(cx, "docker.toggleStoppedContainers", None);
    wait_long(cx, "the stopped container is back", |cx| shows(&view, "container:worker-id", cx));
    close_project(cx);
}

/// Show Untagged Images, run as an action, takes the untagged image out of the Docker tree and, run again,
/// puts it back; the tagged one stays throughout.
#[gpui_kit::test]
async fn show_untagged_images_leaves_the_untagged_image_out_of_the_tree(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = services_project(cx, Vec::new());
    let state = shown_services(&workbench, cx);
    let view = services_view(&workbench, cx);
    let sockets = tempfile::tempdir().expect("a temporary folder");
    let first = add_connection(&state, &sockets.path().join("none.sock"), cx);
    answer(&state, first, snapshot(vec![container("worker", "exited")]), cx);
    wait_long(cx, "both images show", |cx| {
        shows(&view, "image:sha256:pg", cx) && shows(&view, "image:sha256:dangling", cx)
    });

    run_action(cx, "docker.toggleUntaggedImages", None);
    wait_long(cx, "the untagged image goes", |cx| !shows(&view, "image:sha256:dangling", cx));
    assert!(shows(&view, "image:sha256:pg", cx), "the tagged image stays");
    assert!(shows(&view, "container:worker-id", cx), "the containers are left alone");

    run_action(cx, "docker.toggleUntaggedImages", None);
    wait_long(cx, "the untagged image is back", |cx| shows(&view, "image:sha256:dangling", cx));
    close_project(cx);
}
