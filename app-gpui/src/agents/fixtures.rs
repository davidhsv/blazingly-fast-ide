//! Tasks and entries the tests of this feature build on.

use crate::agents::store::{AgentCall, AgentEffect, AgentsStore, Editors, Saved};
use ide_agent::{
    AgentCapabilities, AgentConfigOption, AgentConfigValue, AgentConfigVia, AgentEntry, AgentPermissionOption,
    AgentQuestion, AgentQuestionOption, AgentStatus, AgentSubagentStatus, AgentTask, AgentToolStatus, Isolation,
};
use ide_core::IpcPath;
use std::path::PathBuf;

pub fn path(p: &str) -> IpcPath {
    IpcPath(PathBuf::from(p))
}

/// A worktree task on `agent/<id>` updated at `updated_ms`.
pub fn task(id: &str, status: AgentStatus, updated_ms: f64) -> AgentTask {
    AgentTask {
        project_root: None,
        id: id.to_owned(),
        title: id.to_owned(),
        provider: "claude".to_owned(),
        model: None,
        status,
        isolation: Isolation::Worktree,
        cwd: path("/w"),
        branch: Some(format!("agent/{id}")),
        base: Some("main".to_owned()),
        base_commit: Some("abc".to_owned()),
        error: None,
        created_ms: 0.0,
        updated_ms,
        config: Vec::new(),
        commands: Vec::new(),
        usage: None,
        queue: Vec::new(),
        capabilities: AgentCapabilities::default(),
        resets_ms: None,
        retry: None,
        background: Box::default(),
    }
}

/// A ready task in the local checkout at `/project`, as the session specs use.
pub fn local_task(id: &str) -> AgentTask {
    let mut t = task(id, AgentStatus::Ready, 0.0);
    t.provider = "codex".to_owned();
    t.isolation = Isolation::Local;
    t.cwd = path("/project");
    t.branch = None;
    t.base = None;
    t.base_commit = None;
    t
}

pub fn agent(text: &str) -> AgentEntry {
    AgentEntry::Agent { text: text.to_owned() }
}

pub fn user(text: &str, steered: bool) -> AgentEntry {
    AgentEntry::User {
        text: text.to_owned(),
        attachments: Vec::new(),
        images: Vec::new(),
        sent_ms: Some(0.0),
        done_ms: Some(1.0),
        steered,
    }
}

pub fn thought(text: &str) -> AgentEntry {
    AgentEntry::Thought { text: text.to_owned() }
}

/// A finished `read` tool call titled `id`.
pub fn tool(id: &str) -> AgentEntry {
    tool_with(id, AgentToolStatus::Completed, None)
}

pub fn tool_with(id: &str, status: AgentToolStatus, output: Option<&str>) -> AgentEntry {
    AgentEntry::Tool {
        id: id.to_owned(),
        title: id.to_owned(),
        kind: "read".to_owned(),
        status,
        locations: Vec::new(),
        output: output.map(str::to_owned),
        diffs: Vec::new(),
    }
}

pub fn permission_option(id: &str, name: &str, kind: &str) -> AgentPermissionOption {
    AgentPermissionOption { id: id.to_owned(), name: name.to_owned(), kind: kind.to_owned() }
}

/// An open permission request.
pub fn permission(id: u32, title: &str, options: Vec<AgentPermissionOption>) -> AgentEntry {
    AgentEntry::Permission { id, title: title.to_owned(), options, questions: Vec::new(), chosen: None }
}

pub fn question(header: &str, text: &str, multi_select: bool, options: &[(&str, &str, Option<&str>)]) -> AgentQuestion {
    AgentQuestion {
        answer_key: None,
        header: header.to_owned(),
        question: text.to_owned(),
        multi_select,
        options: options
            .iter()
            .map(|(label, description, preview)| AgentQuestionOption {
                label: (*label).to_owned(),
                description: (*description).to_owned(),
                preview: preview.map(str::to_owned),
            })
            .collect(),
    }
}

/// A finished subagent titled `title`.
pub fn subagent(id: &str, title: &str) -> AgentEntry {
    AgentEntry::Subagent {
        id: id.to_owned(),
        title: Some(title.to_owned()),
        description: None,
        status: AgentSubagentStatus::Done,
        error: None,
        prompt: None,
        reply: None,
        tools: 0,
        activity: None,
        started_ms: 0.0,
        ended_ms: Some(1.0),
        cancellable: false,
    }
}

/// A select setting whose id is its category.
pub fn option(category: &str, values: &[&str], current: Option<&str>) -> AgentConfigOption {
    AgentConfigOption {
        id: category.to_owned(),
        name: category.to_owned(),
        description: None,
        category: Some(category.to_owned()),
        current: current.map(str::to_owned),
        values: values
            .iter()
            .map(|v| AgentConfigValue { value: (*v).to_owned(), name: (*v).to_owned(), description: None })
            .collect(),
        via: AgentConfigVia::Option,
    }
}

/// A store subscribed to a project with `tasks`, and the subscription's generation for its events.
pub fn connected_store(tasks: Vec<AgentTask>) -> (AgentsStore, u64) {
    let mut store = AgentsStore::new(Saved::default());
    let generation = store
        .connect(true)
        .iter()
        .find_map(|e| match e {
            AgentEffect::Subscribe { generation, .. } => Some(*generation),
            _ => None,
        })
        .expect("a subscription");
    store.connected(generation, Ok((tasks, Vec::new())), &Editors::default());
    (store, generation)
}

/// The backend calls among `effects`.
pub fn calls(effects: &[AgentEffect]) -> Vec<AgentCall> {
    effects
        .iter()
        .filter_map(|e| match e {
            AgentEffect::Call { call, .. } => Some(call.clone()),
            _ => None,
        })
        .collect()
}
