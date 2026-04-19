use chrono::Utc;
use codex_core::{
    models::{Board, BoardId, Column, Priority, Task, TaskId, TaskStatus},
    store::{TaskFilter, VaultStore},
};
use dioxus::prelude::*;
use crate::bootstrap::AppContext;

// ── Shared async helpers (avoid move-closure duplication) ────────────────────

async fn create_task(ctx: AppContext, board_id: BoardId, col: String, title: String, project_id: Option<uuid::Uuid>) {
    let vault = ctx.vault();
    let _ = tokio::task::spawn_blocking(move || {
        let task = Task::new(board_id, col, title);
        if let Some(pid) = project_id {
            vault.save_project_task(&task, &pid)
        } else {
            vault.store.save_task(&task)
        }
    })
    .await;
}

async fn move_task(ctx: AppContext, task_id: TaskId, col: String, project_id: Option<uuid::Uuid>) {
    let vault = ctx.vault();
    let _ = tokio::task::spawn_blocking(move || {
        if let Ok(Some(mut t)) = vault.store.get_task(&task_id) {
            t.column     = col;
            t.updated_at = Utc::now();
            if let Some(pid) = project_id {
                vault.save_project_task(&t, &pid)
            } else {
                vault.store.save_task(&t)
            }
        } else {
            Ok(())
        }
    })
    .await;
}

async fn archive_task(ctx: AppContext, task_id: TaskId, project_id: Option<uuid::Uuid>) {
    let vault = ctx.vault();
    let _ = tokio::task::spawn_blocking(move || {
        if let Ok(Some(mut t)) = vault.store.get_task(&task_id) {
            t.status     = TaskStatus::Archived;
            t.updated_at = Utc::now();
            if let Some(pid) = project_id {
                vault.save_project_task(&t, &pid)
            } else {
                vault.store.save_task(&t)
            }
        } else {
            Ok(())
        }
    })
    .await;
}

async fn create_board(ctx: AppContext, name: String) -> anyhow::Result<()> {
    let vault = ctx.vault();
    tokio::task::spawn_blocking(move || {
        vault.store.save_board(&Board::default_sprint(name))
    })
    .await
    .map_err(|e| anyhow::anyhow!("{e}"))??;
    Ok(())
}

// ── Top-level view ────────────────────────────────────────────────────────────

#[component]
pub fn KanbanView() -> Element {
    let ctx = use_context::<AppContext>();
    let refresh = use_signal(|| 0_u64);

    let boards = use_resource(move || {
        let _ = refresh(); // reactive dep
        let vault = ctx.vault();
        async move {
            tokio::task::spawn_blocking(move || vault.store.list_boards().unwrap_or_default())
                .await
                .unwrap_or_default()
        }
    });

    let mut active_board: Signal<Option<BoardId>> = use_signal(|| None);

    use_effect(move || {
        if active_board.read().is_none() {
            if let Some(list) = &*boards.read() {
                if let Some(b) = list.first() {
                    *active_board.write() = Some(b.id.clone());
                }
            }
        }
    });

    rsx! {
        div { class: "view-kanban",
            match &*boards.read() {
                None => rsx! {
                    div { class: "kanban-loading muted", "Loading…" }
                },
                Some(list) if list.is_empty() => rsx! {
                    NewBoardPrompt { refresh }
                },
                Some(list) => rsx! {
                    div { class: "board-tabs",
                        for board in list.iter().cloned() {
                            {
                                let bid = board.id.clone();
                                let is_active = active_board.read().as_ref() == Some(&bid);
                                rsx! {
                                    button {
                                        class: if is_active { "board-tab active" } else { "board-tab" },
                                        onclick: move |_| *active_board.write() = Some(bid.clone()),
                                        "{board.name}"
                                    }
                                }
                            }
                        }
                        NewBoardInline { refresh }
                    }
                    if let Some(board) = list.iter()
                        .find(|b| active_board.read().as_ref() == Some(&b.id))
                        .cloned()
                    {
                        KanbanBoard { board, refresh }
                    }
                },
            }
        }
    }
}

// ── Board ─────────────────────────────────────────────────────────────────────

#[component]
fn KanbanBoard(board: Board, refresh: Signal<u64>) -> Element {
    let ctx     = use_context::<AppContext>();
    let board_id = board.id.clone();
    let project_id = board.project_id;

    let tasks = use_resource(move || {
        let _ = refresh();
        let vault = ctx.vault();
        let bid = board_id.clone();
        async move {
            tokio::task::spawn_blocking(move || {
                vault
                    .store
                    .list_tasks(&TaskFilter { board_id: Some(bid), ..Default::default() })
                    .unwrap_or_default()
            })
            .await
            .unwrap_or_default()
        }
    });

    let dragging: Signal<Option<TaskId>> = use_signal(|| None);

    rsx! {
        div { class: "kanban-board",
            match &*tasks.read() {
                None => rsx! { div { class: "kanban-loading muted", "Loading tasks…" } },
                Some(all) => rsx! {
                    for col in board.columns.iter().cloned() {
                        {
                            let col_tasks: Vec<Task> = all.iter()
                                .filter(|t| t.column == col.name && t.status != TaskStatus::Archived)
                                .cloned()
                                .collect();
                            rsx! {
                                KanbanColumn {
                                    board_id: board.id.clone(),
                                    project_id,
                                    column: col,
                                    tasks: col_tasks,
                                    dragging,
                                    refresh,
                                }
                            }
                        }
                    }
                },
            }
        }
    }
}

// ── Column ────────────────────────────────────────────────────────────────────

#[component]
fn KanbanColumn(
    board_id:   BoardId,
    project_id: Option<uuid::Uuid>,
    column:     Column,
    tasks:      Vec<Task>,
    dragging:   Signal<Option<TaskId>>,
    mut refresh: Signal<u64>,
) -> Element {
    let ctx         = use_context::<AppContext>();
    let col_name    = column.name.clone();
    let count       = tasks.len();
    let over_wip    = column.wip_limit.map(|l| count > l as usize).unwrap_or(false);
    let wip_label   = match column.wip_limit {
        Some(l) => format!("{count}/{l}"),
        None    => format!("{count}"),
    };

    let mut adding    = use_signal(|| false);
    let mut new_title = use_signal(String::new);
    let mut drag_over = use_signal(|| false);

    let ctx_drop = ctx.clone();
    let col_drop = col_name.clone();

    // Add task — shared inline logic, duplicated per handler to avoid move issues.
    // (Signals are Copy; only String/Arc values need cloning.)
    let ctx_add1  = ctx.clone();
    let col_add1  = col_name.clone();
    let bid_add1  = board_id.clone();
    let ctx_add2  = ctx.clone();
    let col_add2  = col_name.clone();
    let bid_add2  = board_id.clone();

    let do_add_onclick = move |_| {
        let title = new_title.read().trim().to_string();
        if title.is_empty() { return; }
        let c = ctx_add1.clone();
        let col = col_add1.clone();
        let bid = bid_add1.clone();
        spawn(async move {
            create_task(c, bid, col, title, project_id).await;
            *refresh.write() += 1;
        });
        *new_title.write() = String::new();
        *adding.write() = false;
    };

    let do_add_keydown = move |e: Event<KeyboardData>| {
        match e.key() {
            Key::Enter => {
                let title = new_title.read().trim().to_string();
                if title.is_empty() { return; }
                let c = ctx_add2.clone();
                let col = col_add2.clone();
                let bid = bid_add2.clone();
                spawn(async move {
                    create_task(c, bid, col, title, project_id).await;
                    *refresh.write() += 1;
                });
                *new_title.write() = String::new();
                *adding.write() = false;
            }
            Key::Escape => {
                *new_title.write() = String::new();
                *adding.write() = false;
            }
            _ => {}
        }
    };

    let col_class = match (over_wip, *drag_over.read()) {
        (true, true)   => "kanban-column over-wip drag-over",
        (true, false)  => "kanban-column over-wip",
        (false, true)  => "kanban-column drag-over",
        (false, false) => "kanban-column",
    };

    rsx! {
        div {
            class: col_class,
            ondragover: move |e| e.prevent_default(),
            ondragenter: move |_| drag_over.set(true),
            ondragleave: move |_| drag_over.set(false),
            ondrop: move |e: Event<DragData>| {
                e.prevent_default();
                drag_over.set(false);
                let Some(tid) = dragging.read().clone() else { return };
                let c = ctx_drop.clone();
                let col = col_drop.clone();
                spawn(async move {
                    move_task(c, tid, col, project_id).await;
                    *refresh.write() += 1;
                });
                *dragging.write() = None;
            },

            div { class: "kanban-column-header",
                span { class: "kanban-column-name", "{col_name}" }
                span { class: if over_wip { "kanban-wip over" } else { "kanban-wip" }, "{wip_label}" }
            }

            div { class: "kanban-column-body",
                for task in tasks.iter().cloned() {
                    TaskCard { task, project_id, dragging, refresh }
                }

                if *adding.read() {
                    div { class: "new-task-card",
                        input {
                            autofocus: true,
                            value: "{new_title}",
                            placeholder: "Task title…",
                            oninput: move |e| *new_title.write() = e.value(),
                            onkeydown: do_add_keydown,
                        }
                        div { class: "row gap-2",
                            button { class: "btn btn-primary", onclick: do_add_onclick, "Add" }
                            button {
                                class: "btn btn-ghost",
                                onclick: move |_| {
                                    *adding.write() = false;
                                    *new_title.write() = String::new();
                                },
                                "Cancel"
                            }
                        }
                    }
                } else {
                    button {
                        class: "add-task-btn",
                        onclick: move |_| *adding.write() = true,
                        "+ Add task"
                    }
                }
            }
        }
    }
}

// ── Task card ────────────────────────────────────────────────────────────────

#[component]
fn TaskCard(task: Task, project_id: Option<uuid::Uuid>, dragging: Signal<Option<TaskId>>, mut refresh: Signal<u64>) -> Element {
    let ctx = use_context::<AppContext>();
    let mut open = use_signal(|| false);
    let mut inline_title = use_signal(|| task.title.clone());
    let mut inline_desc = use_signal(|| task.description.clone());
    let mut inline_priority = use_signal(|| task.priority);
    let mut inline_due = use_signal(|| task.due_date.map(|d| d.to_string()).unwrap_or_default());

    let tid_drag = task.id.clone();
    let tid_archive = task.id.clone();
    let ctx_archive = ctx.clone();
    let priority_class = priority_badge_class(task.priority);

    rsx! {
        div {
            class: "task-card",
            draggable: true,
            ondragstart: move |_| *dragging.write() = Some(tid_drag.clone()),
            ondragend:   move |_| *dragging.write() = None,

            div { class: "task-card-top",
                div { class: "task-priority {priority_class}" }
                div { class: "task-title", "{task.title}" }
                if !task.tags.is_empty() {
                    div { class: "task-tags-inline",
                        for tag in task.tags.iter() {
                            span { class: "task-tag", "{tag}" }
                        }
                    }
                }
                button {
                    class: "task-menu-btn",
                    title: if *open.read() { "Close details" } else { "Open details" },
                    onclick: move |_| {
                        let is_open = *open.read();
                        *open.write() = !is_open;
                    },
                    if *open.read() { "▾" } else { "▸" }
                }
            }

            if *open.read() {
                div { class: "task-details",
                    label { class: "field",
                        span { "Title" }
                        input {
                            value: "{inline_title}",
                            oninput: move |e| *inline_title.write() = e.value(),
                        }
                    }

                    label { class: "field",
                        span { "Description" }
                        textarea {
                            class: "task-desc-input",
                            value: "{inline_desc}",
                            rows: "3",
                            placeholder: "Task description…",
                            oninput: move |e| *inline_desc.write() = e.value(),
                        }
                    }

                    div { class: "task-detail-row",
                        label { class: "field",
                            span { "Priority" }
                            select {
                                value: "{priority_to_str(*inline_priority.read())}",
                                onchange: move |e| {
                                    *inline_priority.write() = str_to_priority(&e.value());
                                },
                                option { value: "low", "Low" }
                                option { value: "medium", "Medium" }
                                option { value: "high", "High" }
                                option { value: "critical", "Critical" }
                            }
                        }

                        label { class: "field",
                            span { "Due date" }
                            input {
                                r#type: "date",
                                value: "{inline_due}",
                                oninput: move |e| *inline_due.write() = e.value(),
                            }
                        }
                    }

                    div { class: "row gap-2",
                        button {
                            class: "btn btn-primary",
                            onclick: move |_| {
                                let c = ctx.clone();
                                let task_id = task.id.clone();
                                let new_title = inline_title.read().trim().to_string();
                                let new_desc = inline_desc.read().clone();
                                let new_priority = *inline_priority.read();
                                let new_due = inline_due.read().clone();
                                spawn(async move {
                                    let vault = c.vault();
                                    let _ = tokio::task::spawn_blocking(move || {
                                        if let Ok(Some(mut t)) = vault.store.get_task(&task_id) {
                                            t.title      = new_title;
                                            t.description = new_desc;
                                            t.priority   = new_priority;
                                            t.due_date   = chrono::NaiveDate::parse_from_str(&new_due, "%Y-%m-%d").ok();
                                            t.updated_at = Utc::now();
                                            if let Some(pid) = project_id {
                                                vault.save_project_task(&t, &pid)
                                            } else {
                                                vault.store.save_task(&t)
                                            }
                                        } else {
                                            Ok(())
                                        }
                                    }).await;
                                    *refresh.write() += 1;
                                });
                            },
                            "Save"
                        }

                        button {
                            class: "btn btn-ghost",
                            onclick: move |_| {
                                let c = ctx_archive.clone();
                                let task_id = tid_archive.clone();
                                spawn(async move {
                                    archive_task(c, task_id, project_id).await;
                                    *refresh.write() += 1;
                                });
                            },
                            "Archive"
                        }
                    }
                }
            }
        }
    }
}

fn priority_to_str(p: Priority) -> &'static str {
    match p {
        Priority::Low => "low",
        Priority::Medium => "medium",
        Priority::High => "high",
        Priority::Critical => "critical",
    }
}

fn str_to_priority(s: &str) -> Priority {
    match s {
        "low" => Priority::Low,
        "high" => Priority::High,
        "critical" => Priority::Critical,
        _ => Priority::Medium,
    }
}

fn priority_badge_class(priority: Priority) -> &'static str {
    match priority {
        Priority::Low => "low",
        Priority::Medium => "medium",
        Priority::High => "high",
        Priority::Critical => "critical",
    }
}

// ── New board prompts ────────────────────────────────────────────────────────

#[component]
fn NewBoardPrompt(mut refresh: Signal<u64>) -> Element {
    let ctx = use_context::<AppContext>();
    let ctx2 = ctx.clone();
    let mut name = use_signal(|| "Sprint 1".to_string());
    let mut error_msg: Signal<Option<String>> = use_signal(|| None);

    let do_create = move |_| {
        let n = name.read().trim().to_string();
        if n.is_empty() { return; }
        let c = ctx.clone();
        spawn(async move {
            match create_board(c, n).await {
                Ok(()) => *refresh.write() += 1,
                Err(e) => *error_msg.write() = Some(format!("{e}")),
            }
        });
        *name.write() = String::new();
    };

    rsx! {
        div { class: "new-board-prompt",
            h2 { class: "view-heading", "Create your first board" }
            p { class: "muted", "Boards organize tasks into columns like Backlog, In Progress, Review, and Done." }
            div { class: "row gap-2",
                input {
                    autofocus: true,
                    value: "{name}",
                    placeholder: "Board name…",
                    oninput: move |e| *name.write() = e.value(),
                    onkeydown: move |e| {
                        if e.key() == Key::Enter {
                            let n = name.read().trim().to_string();
                            if n.is_empty() { return; }
                            let c = ctx2.clone();
                            spawn(async move {
                                match create_board(c, n).await {
                                    Ok(()) => *refresh.write() += 1,
                                    Err(e) => *error_msg.write() = Some(format!("{e}")),
                                }
                            });
                            *name.write() = String::new();
                        }
                    },
                }
                button {
                    class: "btn btn-primary",
                    onclick: do_create,
                    "Create board"
                }
            }
            if let Some(ref err) = *error_msg.read() {
                p { class: "text-error", "Error: {err}" }
            }
        }
    }
}

#[component]
fn NewBoardInline(mut refresh: Signal<u64>) -> Element {
    let ctx      = use_context::<AppContext>();
    let mut open = use_signal(|| false);
    let mut name = use_signal(String::new);

    rsx! {
        if *open.read() {
            div { class: "board-new-inline",
                input {
                    autofocus: true,
                    value: "{name}",
                    placeholder: "New board…",
                    oninput: move |e| *name.write() = e.value(),
                    onkeydown: move |e| {
                        match e.key() {
                            Key::Enter => {
                                let n = name.read().trim().to_string();
                                if n.is_empty() { return; }
                                let c = ctx.clone();
                                spawn(async move {
                                    if create_board(c, n).await.is_ok() {
                                        *refresh.write() += 1;
                                    }
                                });
                                *name.write() = String::new();
                                *open.write() = false;
                            }
                            Key::Escape => {
                                *name.write() = String::new();
                                *open.write() = false;
                            }
                            _ => {}
                        }
                    },
                }
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| {
                        *name.write() = String::new();
                        *open.write() = false;
                    },
                    "Cancel"
                }
            }
        } else {
            button {
                class: "board-tab new",
                onclick: move |_| *open.write() = true,
                "+ New board"
            }
        }
    }
}
