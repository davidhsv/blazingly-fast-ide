//! Remote ports forwarded to this machine, remembered per host (or dev container) and folder and
//! forwarded again on reconnect. A dev container also forwards its `forwardPorts` on its own.
//!
//! [`Ports`] follows the window's remote session ([`Ports::session_changed`]): the Ports tool
//! window is there while the window is connected to an SSH host or a dev container. Every backend
//! call is a [`PortsRequest`] the caller carries out (`remote_forward_port`,
//! `remote_stop_forwarding`, `remote_forwarded_ports`) and answers with [`Ports::answered`]; an
//! answer for a session the window has left is dropped.

use crate::platform::i18n::{I18n, Param};
use crate::platform::storage::UiStorage;
use crate::platform_more::remote::{RemoteSession, RemoteTarget};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_core::IdeError;
use serde_json::{Value, json};

const SAVED_KEY: &str = "openidea.remote.ports:";

/// A port of the remote host reachable on this machine at `localhost:<local_port>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForwardedPort {
    pub remote_port: u16,
    pub local_port: u16,
}

/// Where a forwarded port is reachable here.
pub fn address(port: &ForwardedPort) -> String {
    format!("localhost:{}", port.local_port)
}

/// The URL "Open in Browser" opens for a forwarded port.
pub fn browser_url(port: &ForwardedPort) -> String {
    format!("http://{}/", address(port))
}

/// A port number typed by the user: digits only, 1 to 65535.
pub fn parse_port(text: &str) -> Option<u16> {
    let text = text.trim();
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse::<u32>().ok().filter(|port| (1..=65535).contains(port)).and_then(|port| u16::try_from(port).ok())
}

/// The storage key of the ports remembered for `session`: per SSH configuration and folder, or
/// per dev container (local folder and configuration). `None` for no session or a WSL one.
fn saved_key(session: Option<&RemoteSession>) -> Option<String> {
    let session = session?;
    match &session.target {
        RemoteTarget::Ssh { config } => Some(format!("{SAVED_KEY}{config}:{}", session.root)),
        RemoteTarget::DevContainer { folder, config } => {
            Some(format!("{SAVED_KEY}devcontainer:{}", json!([folder, config])))
        }
        RemoteTarget::Wsl { .. } => None,
    }
}

/// The remote ports remembered for `session`.
pub fn saved_ports(storage: &UiStorage, session: Option<&RemoteSession>) -> Vec<u16> {
    let Some(text) = saved_key(session).and_then(|key| storage.get(&key).map(str::to_owned)) else {
        return Vec::new();
    };
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&text) else { return Vec::new() };
    items
        .iter()
        .filter_map(Value::as_f64)
        .filter(|port| port.fract() == 0.0 && *port > 0.0 && *port < 65536.0)
        .map(|port| port as u16)
        .collect()
}

fn save(storage: &mut UiStorage, session: Option<&RemoteSession>, ports: &[ForwardedPort]) {
    let Some(key) = saved_key(session) else { return };
    if ports.is_empty() {
        storage.remove(&key);
    } else {
        let remote: Vec<u16> = ports.iter().map(|port| port.remote_port).collect();
        storage.set(&key, json!(remote).to_string());
    }
}

/// A backend call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortsCall {
    /// `remote_forward_port`, answered with [`PortsAnswer::Forwarded`].
    Forward(u16),
    /// `remote_stop_forwarding`, answered with [`PortsAnswer::Stopped`].
    StopForwarding(u16),
    /// `remote_forwarded_ports`, answered with [`PortsAnswer::Listed`].
    List,
}

/// The answer to a [`PortsCall`].
#[derive(Debug, Clone)]
pub enum PortsAnswer {
    Forwarded(Result<ForwardedPort, IdeError>),
    /// Whether stopping succeeded does not matter: the list is read again either way.
    Stopped,
    /// A failure reads as no forwarded ports.
    Listed(Result<Vec<ForwardedPort>, IdeError>),
}

/// What an answer continues.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Forward {
        port: u16,
    },
    Stop,
    /// `forwarded`: the port a forward just forwarded, reported once the list is read.
    Refresh {
        remember: bool,
        forwarded: Option<ForwardedPort>,
    },
    /// Forwarding the remembered ports again; `remaining` are still to forward.
    Restore {
        remaining: Vec<u16>,
    },
    RestoreList,
}

/// Which request an answer belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortsTicket {
    generation: u64,
    step: Step,
}

/// A call to carry out; its answer goes to [`Ports::answered`] with the ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortsRequest {
    pub ticket: PortsTicket,
    pub call: PortsCall,
}

/// What the caller does.
#[derive(Debug, Clone, PartialEq)]
pub enum PortsEffect {
    Call(PortsRequest),
    Notify(NotificationRequest),
    /// [`Ports::forward_port`] finished: the forwarded port, or `None` when it was not forwarded
    /// (the view keeps what was typed).
    ForwardDone(Option<ForwardedPort>),
    /// Register the Ports tool window ([`super::PORTS_TOOL_WINDOW`]).
    ShowToolWindow,
    /// Unregister the Ports tool window.
    HideToolWindow,
}

/// The forwarded ports of the window's remote session.
#[derive(Debug, Clone, Default)]
pub struct Ports {
    session: Option<RemoteSession>,
    /// Bumped whenever the session changes: answers of an older generation are dropped.
    generation: u64,
    ports: Vec<ForwardedPort>,
    tool_window: bool,
}

impl Ports {
    pub fn new() -> Self {
        Self::default()
    }

    /// The ports forwarded now, as the backend last listed them.
    pub fn ports(&self) -> &[ForwardedPort] {
        &self.ports
    }

    pub fn session(&self) -> Option<&RemoteSession> {
        self.session.as_ref()
    }

    /// Whether the Ports tool window is registered.
    pub fn tool_window_shown(&self) -> bool {
        self.tool_window
    }

    /// The remote ports remembered for the current session.
    pub fn saved_ports(&self, storage: &UiStorage) -> Vec<u16> {
        saved_ports(storage, self.session.as_ref())
    }

    fn call(generation: u64, call: PortsCall, step: Step) -> PortsEffect {
        PortsEffect::Call(PortsRequest { ticket: PortsTicket { generation, step }, call })
    }

    /// The window's remote session is now `session`. Connected to an SSH host or a dev container
    /// (newly, or to another one) the list starts empty and the remembered ports are forwarded
    /// again; the tool window comes and goes with the connection. Sessions are compared by value:
    /// a reconnect that yields an equal session forwards nothing again until [`Ports::restore`].
    pub fn session_changed(&mut self, session: Option<RemoteSession>, storage: &UiStorage) -> Vec<PortsEffect> {
        let connected = matches!(
            session.as_ref().map(|session| &session.target),
            Some(RemoteTarget::Ssh { .. } | RemoteTarget::DevContainer { .. })
        );
        let changed = session != self.session;
        if changed {
            self.generation += 1;
            self.session = session;
        }
        let mut effects = Vec::new();
        if connected && (changed || !self.tool_window) {
            self.ports.clear();
            effects.extend(self.restore(storage));
        }
        if connected && !self.tool_window {
            self.tool_window = true;
            effects.push(PortsEffect::ShowToolWindow);
        } else if !connected && self.tool_window {
            self.tool_window = false;
            self.ports.clear();
            effects.push(PortsEffect::HideToolWindow);
        }
        effects
    }

    /// Reads the forwarded ports; `remember` keeps them for the next connection (after the user
    /// changed them).
    pub fn refresh(&self, remember: bool) -> PortsEffect {
        Self::call(self.generation, PortsCall::List, Step::Refresh { remember, forwarded: None })
    }

    /// Forwards remote port `port`. A WSL project cannot forward ports yet.
    pub fn forward_port(&self, port: u16, i18n: &I18n) -> Vec<PortsEffect> {
        if matches!(self.session.as_ref().map(|session| &session.target), Some(RemoteTarget::Wsl { .. })) {
            let title = i18n.tx("Forwarding ports is not supported in a WSL project yet");
            let mut request = NotificationRequest::new(NotificationLevel::Error, title);
            request.group = Some("remote".to_owned());
            return vec![PortsEffect::Notify(request), PortsEffect::ForwardDone(None)];
        }
        vec![Self::call(self.generation, PortsCall::Forward(port), Step::Forward { port })]
    }

    pub fn stop_forwarding(&self, port: u16) -> PortsEffect {
        Self::call(self.generation, PortsCall::StopForwarding(port), Step::Stop)
    }

    /// After (re)connecting: forwards the ports this host and folder had forwarded before, one
    /// after the other, then reads the list.
    pub fn restore(&self, storage: &UiStorage) -> Vec<PortsEffect> {
        vec![Self::restore_step(self.generation, self.saved_ports(storage))]
    }

    fn restore_step(generation: u64, mut remaining: Vec<u16>) -> PortsEffect {
        if remaining.is_empty() {
            return Self::call(generation, PortsCall::List, Step::RestoreList);
        }
        let port = remaining.remove(0);
        Self::call(generation, PortsCall::Forward(port), Step::Restore { remaining })
    }

    /// The backend answered the request of `ticket`.
    pub fn answered(
        &mut self,
        ticket: PortsTicket,
        answer: PortsAnswer,
        storage: &mut UiStorage,
        i18n: &mut I18n,
    ) -> Vec<PortsEffect> {
        let current = ticket.generation == self.generation;
        match (ticket.step, answer) {
            (Step::Forward { .. }, PortsAnswer::Forwarded(Ok(forwarded))) => {
                let refresh = Step::Refresh { remember: true, forwarded: Some(forwarded) };
                vec![Self::call(ticket.generation, PortsCall::List, refresh)]
            }
            (Step::Forward { port }, PortsAnswer::Forwarded(Err(error))) => {
                let title = i18n.tx_built(&["Could not forward port ", ""], &[Param::from(f64::from(port))]);
                let mut request = NotificationRequest::new(NotificationLevel::Error, title);
                request.body = Some(error.message);
                request.group = Some("remote".to_owned());
                vec![PortsEffect::Notify(request), PortsEffect::ForwardDone(None)]
            }
            (Step::Stop, PortsAnswer::Stopped) => {
                let refresh = Step::Refresh { remember: true, forwarded: None };
                vec![Self::call(ticket.generation, PortsCall::List, refresh)]
            }
            (Step::Refresh { remember, forwarded }, PortsAnswer::Listed(listed)) => {
                if current {
                    self.ports = listed.unwrap_or_default();
                    if remember {
                        save(storage, self.session.as_ref(), &self.ports);
                    }
                }
                forwarded.map(|port| PortsEffect::ForwardDone(Some(port))).into_iter().collect()
            }
            (Step::Restore { remaining }, PortsAnswer::Forwarded(_)) if current => {
                vec![Self::restore_step(ticket.generation, remaining)]
            }
            (Step::RestoreList, PortsAnswer::Listed(listed)) if current => {
                self.ports = listed.unwrap_or_default();
                Vec::new()
            }
            _ => Vec::new(),
        }
    }
}

/// The Ports tool window's "Remote port" field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PortField {
    text: String,
}

impl PortField {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_text(&mut self, text: &str) {
        text.clone_into(&mut self.text);
    }

    /// The port typed, if it is one.
    pub fn port(&self) -> Option<u16> {
        parse_port(&self.text)
    }

    /// Whether "Forward Port" is enabled.
    pub fn can_forward(&self) -> bool {
        self.port().is_some()
    }

    /// Whether the field shows what was typed as invalid (something that is not a port).
    pub fn invalid(&self) -> bool {
        !self.text.trim().is_empty() && self.port().is_none()
    }

    /// A forward finished: a forwarded port clears the field, a failure keeps what was typed.
    pub fn forward_done(&mut self, forwarded: Option<&ForwardedPort>) {
        if forwarded.is_some() {
            self.text.clear();
        }
    }
}

/// The tooltip of a port's "Stop Forwarding" button.
pub fn stop_title(port: &ForwardedPort, i18n: &mut I18n) -> String {
    i18n.tx_built(&["Stop Forwarding Port ", ""], &[Param::from(f64::from(port.remote_port))])
}

/// The tooltip of a port's "Open in Browser" button.
pub fn open_title(port: &ForwardedPort, i18n: &mut I18n) -> String {
    i18n.tx_built(&["Open ", " in Browser"], &[Param::from(address(port))])
}

/// The tooltip of a port's "Copy" button.
pub fn copy_title(port: &ForwardedPort, i18n: &mut I18n) -> String {
    i18n.tx_built(&["Copy ", ""], &[Param::from(address(port))])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The backend: forwards remote port 3000 to local port 53000 (3000 is taken here), every
    /// other port to the same port.
    #[derive(Default)]
    struct Backend {
        forwarded: Vec<ForwardedPort>,
        forward_calls: Vec<u16>,
    }

    impl Backend {
        fn answer(&mut self, call: PortsCall) -> PortsAnswer {
            match call {
                PortsCall::Forward(remote_port) => {
                    self.forward_calls.push(remote_port);
                    let local_port = if remote_port == 3000 { 53000 } else { remote_port };
                    let port = ForwardedPort { remote_port, local_port };
                    self.forwarded.push(port);
                    PortsAnswer::Forwarded(Ok(port))
                }
                PortsCall::StopForwarding(remote_port) => {
                    self.forwarded.retain(|port| port.remote_port != remote_port);
                    PortsAnswer::Stopped
                }
                PortsCall::List => PortsAnswer::Listed(Ok(self.forwarded.clone())),
            }
        }
    }

    struct Harness {
        ports: Ports,
        backend: Backend,
        storage: UiStorage,
        i18n: I18n,
    }

    impl Harness {
        fn new() -> Self {
            Self {
                ports: Ports::new(),
                backend: Backend::default(),
                storage: UiStorage::default(),
                i18n: I18n::default(),
            }
        }

        /// Carries out every call, answering each at once; returns the other effects.
        fn drive(&mut self, effects: Vec<PortsEffect>) -> Vec<PortsEffect> {
            let mut queue = std::collections::VecDeque::from(effects);
            let mut rest = Vec::new();
            while let Some(effect) = queue.pop_front() {
                match effect {
                    PortsEffect::Call(request) => {
                        let answer = self.backend.answer(request.call);
                        queue.extend(self.ports.answered(request.ticket, answer, &mut self.storage, &mut self.i18n));
                    }
                    other => rest.push(other),
                }
            }
            rest
        }

        fn connect(&mut self, session: Option<RemoteSession>) -> Vec<PortsEffect> {
            let effects = self.ports.session_changed(session, &self.storage);
            self.drive(effects)
        }

        fn forward(&mut self, port: u16) -> Vec<PortsEffect> {
            let effects = self.ports.forward_port(port, &self.i18n);
            self.drive(effects)
        }

        fn saved(&self) -> Vec<u16> {
            self.ports.saved_ports(&self.storage)
        }
    }

    fn session(target: RemoteTarget, root: &str) -> RemoteSession {
        RemoteSession {
            target,
            root: root.to_owned(),
            host: "h".to_owned(),
            os: "linux".to_owned(),
            commands: Vec::new(),
        }
    }

    fn ssh(config: &str, root: &str) -> RemoteSession {
        session(RemoteTarget::Ssh { config: config.to_owned() }, root)
    }

    fn container(folder: &str, config: &str, root: &str) -> RemoteSession {
        session(RemoteTarget::DevContainer { folder: folder.to_owned(), config: config.to_owned() }, root)
    }

    fn port(remote_port: u16, local_port: u16) -> ForwardedPort {
        ForwardedPort { remote_port, local_port }
    }

    // Ports tool window › accepts port numbers 1–65535 only
    #[test]
    fn accepts_port_numbers_1_65535_only() {
        assert_eq!(parse_port(" 8080 "), Some(8080));
        assert_eq!([parse_port("0"), parse_port("65536"), parse_port("80a"), parse_port("")], [None; 4]);
    }

    // Ports tool window › forwards a typed port, shows where it is reachable here, and stops it
    #[test]
    fn forwards_a_typed_port_shows_where_it_is_reachable_here_and_stops_it() {
        let mut harness = Harness::new();
        assert!(harness.ports.ports().is_empty());
        let mut field = PortField::default();
        field.set_text("70000");
        assert!(!field.can_forward() && field.invalid());
        field.set_text("3000");
        let typed = field.port().expect("a port");
        let done = harness.forward(typed);
        assert_eq!(done, [PortsEffect::ForwardDone(Some(port(3000, 53000)))]);
        let shown: Vec<String> = harness.ports.ports().iter().map(address).collect();
        assert_eq!(shown, ["localhost:53000"]);
        assert_eq!(harness.backend.forward_calls, [3000]);
        field.forward_done(Some(&port(3000, 53000)));
        assert_eq!(field.text(), "");
        assert_eq!(stop_title(&harness.ports.ports()[0], &mut harness.i18n), "Stop Forwarding Port 3000");
        let stop = harness.ports.stop_forwarding(3000);
        harness.drive(vec![stop]);
        assert!(harness.ports.ports().is_empty());
    }

    // restoring forwarded ports › forwards the ports of this host and folder again after reconnecting
    #[test]
    fn forwards_the_ports_of_this_host_and_folder_again_after_reconnecting() {
        let mut harness = Harness::new();
        harness.connect(Some(ssh("lh", "/srv/app")));
        harness.forward(8080);
        assert_eq!(harness.saved(), [8080]);
        // Reconnected: the new connection forwards nothing yet.
        harness.backend.forwarded.clear();
        harness.backend.forward_calls.clear();
        let restore = harness.ports.restore(&harness.storage);
        harness.drive(restore);
        assert_eq!(harness.backend.forward_calls, [8080]);
        assert_eq!(harness.ports.ports(), [port(8080, 8080)]);
        // Another folder on the same host has its own list.
        harness.connect(Some(ssh("lh", "/srv/other")));
        assert!(harness.saved().is_empty());
        harness.connect(Some(session(RemoteTarget::Wsl { distro: "lh".to_owned() }, "/srv/app")));
        assert!(harness.saved().is_empty());
    }

    // forwards and remembers ports of a dev container, per folder and configuration
    #[test]
    fn forwards_and_remembers_ports_of_a_dev_container_per_folder_and_configuration() {
        let mut harness = Harness::new();
        harness.connect(Some(container("/w/app", "", "/workspaces/app")));
        assert_eq!(harness.forward(8000), [PortsEffect::ForwardDone(Some(port(8000, 8000)))]);
        assert_eq!(harness.backend.forward_calls, [8000]);
        assert_eq!(harness.saved(), [8000]);
        // The key is the local folder and the configuration, not the path in the container.
        let elsewhere = container("/w/app", "", "/another-workspace");
        assert_eq!(saved_ports(&harness.storage, Some(&elsewhere)), [8000]);
        let other = container("/w/app", ".devcontainer/other/devcontainer.json", "/workspaces/app");
        assert!(saved_ports(&harness.storage, Some(&other)).is_empty());
    }

    // doesn't forward ports of a WSL project yet
    #[test]
    fn doesnt_forward_ports_of_a_wsl_project_yet() {
        let mut harness = Harness::new();
        let shown = harness.connect(Some(session(RemoteTarget::Wsl { distro: "u".to_owned() }, "/home/u")));
        assert!(shown.is_empty());
        let effects = harness.ports.forward_port(8080, &harness.i18n);
        assert!(!effects.iter().any(|effect| matches!(effect, PortsEffect::Call(_))));
        assert!(effects.contains(&PortsEffect::ForwardDone(None)));
        let error = |effect: &PortsEffect| matches!(effect, PortsEffect::Notify(note) if note.level == NotificationLevel::Error);
        assert!(effects.iter().any(error));
        assert!(harness.backend.forward_calls.is_empty());
    }

    // restores manually forwarded container ports alongside configured ports
    #[test]
    fn restores_manually_forwarded_container_ports_alongside_configured_ports() {
        let mut harness = Harness::new();
        harness.connect(Some(container("/w/app", "", "/workspaces/app")));
        harness.forward(9000);
        assert_eq!(harness.saved(), [9000]);
        // The container forwards its configured port on its own.
        harness.backend.forwarded = vec![port(8000, 8000)];
        let restore = harness.ports.restore(&harness.storage);
        harness.drive(restore);
        assert_eq!(harness.ports.ports(), [port(8000, 8000), port(9000, 9000)]);
        let other = container("/w/other", "", "/workspaces/app");
        assert!(saved_ports(&harness.storage, Some(&other)).is_empty());
    }

    // does not display a previous session's delayed port response
    #[test]
    fn does_not_display_a_previous_sessions_delayed_port_response() {
        let mut harness = Harness::new();
        let PortsEffect::Call(pending) = harness.ports.refresh(true) else { panic!("a list request") };
        let _ = harness.ports.session_changed(Some(container("/other", "", "/workspaces/other")), &harness.storage);
        let late = PortsAnswer::Listed(Ok(vec![port(8000, 8000)]));
        let effects = harness.ports.answered(pending.ticket, late, &mut harness.storage, &mut harness.i18n);
        assert!(effects.is_empty());
        assert!(harness.ports.ports().is_empty());
    }

    // reloads ports when switching directly between connected sessions
    #[test]
    fn reloads_ports_when_switching_directly_between_connected_sessions() {
        let mut harness = Harness::new();
        harness.backend.forwarded = vec![port(8000, 8000)];
        let first = harness.connect(Some(container("/first", "", "/workspaces/app")));
        assert_eq!(first, [PortsEffect::ShowToolWindow]);
        assert_eq!(harness.ports.ports(), [port(8000, 8000)]);
        harness.backend.forwarded = vec![port(9000, 9000)];
        let second = harness.connect(Some(container("/second", "", "/workspaces/app")));
        assert!(second.is_empty());
        assert_eq!(harness.ports.ports(), [port(9000, 9000)]);
        assert_eq!(harness.connect(None), [PortsEffect::HideToolWindow]);
        assert!(harness.ports.ports().is_empty() && !harness.ports.tool_window_shown());
    }
}
