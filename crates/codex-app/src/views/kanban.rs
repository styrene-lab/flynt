use chrono::Utc;
use codex_core::{
    models::{Board, BoardId, Column, Priority, Task, TaskId, TaskStatus},
    store::{TaskFilter, VaultStore},
};
use dioxus::prelude::*;
use crate::bootstrap::AppContext;

// ── Shared async helpers (avoid move-closure duplication) ────────────────────

async fn create_task(ctx: AppContext, board_id: BoardId, col: String, title: String) {
    let _ = tokio::task::spawn_blocking(move || {
        ctx.vault.store.save_task(&Task::new(board_id, col, title))
    })
    .await;
}

async fn move_task(ctx: AppContext, task_id: TaskId, col: String) {
    let _ = tokio::task::spawn_blocking(move || {
        if let Ok(Some(mut t)) = ctx.vault.store.get_task(&task_id) {
            t.column     = col;
            t.updated_at = Utc::now();
            ctx.vault.store.save_task(&t)
        } else {
            Ok(())
        }
    })
    .await;
}

async fn archive_task(ctx: AppContext, task_id: TaskId) {
    let _ = tokio::task::spawn_blocking(move || {
        if let Ok(Some(mut t)) = ctx.vault.store.get_task(&task_id) {
            t.status     = TaskStatus::Archived;
            t.updated_at = Utc::now();
            ctx.vault.store.save_task(&t)
        } else {
            Ok(())
        }
    })
    .await;
}

async fn create_board(ctx: AppContext, name: String) {
    let _ = tokio::task::spawn_blocking(move || {
        ctx.vault.store.save_board(&Board::default_sprint(name))
    })
    .await;
}

// ── Top-level view ────────────────────────────────────────────────────────────

#[component]
pub fn KanbanView() -> Element {
    let ctx = use_context::<AppContext>();
    let mut refresh = use_signal(|| 0_u64);

    let boards = use_resource(move || {
        let _ = refresh(); // reactive dep
        let c = ctx.clone();
        async move {
            tokio::task::spawn_blocking(move || c.vault.store.list_boards().unwrap_or_default())
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

    let tasks = use_resource(move || {
        let _ = refresh();
        let c   = ctx.clone();
        let bid = board_id.clone();
        async move {
            tokio::task::spawn_blocking(move || {
                c.vault
                    .store
                    .list_tasks(&TaskFilter { board_id: Some(bid), ..Default::default() })
                    .unwrap_or_default()
            })
            .await
            .unwrap_or_default()
        }
    });

    let mut dragging: Signal<Option<TaskId>> = use_signal(|| None);

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
    board_id:  BoardId,
    column:    Column,
    tasks:     Vec<Task>,
    dragging:  Signal<Option<TaskId>>,
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

    // Drop: move dragged card into this column.
    let ctx_drop = ctx.clone();
    let col_drop = col_name.clone();
    let on_drop  = move |e: Event<DragData>| {
        e.prevent_default();
        let Some(tid) = dragging.read().clone() else { return };
        let c = ctx_drop.clone();
        let col = col_drop.clone();
        spawn(async move {
            move_task(c, tid, col).await;
            *refresh.write() += 1;
        });
        *dragging.write() = None;
    };

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
            create_task(c, bid, col, title).await;
            *refresh.write() += 1;
        });
        *new_title.write() = String::new();
        *adding.write() = false;
    };

    let do_add_keydown = move |e: Event<KeyboardData>| {
        match e.key() {
            Key::Enter if !e.modifiers().shift() => {
                let title = new_title.read().trim().to_string();
                if title.is_empty() { return; }
                let c   = ctx_add2.clone();
                let col = col_add2.clone();
                let bid = bid_add2.clone();
                spawn(async move {
                    create_task(c, bid, col, title).await;
                    *refresh.write() += 1;
                });
                *new_title.write() = String::new();
                *adding.write() = false;
            }
            Key::Escape => {
                *adding.write() = false;
                *new_title.write() = String::new();
            }
            _ => {}
        }
    };

    rsx! {
        div {
            class: "kanban-column",
            ondragover: move |e: Event<DragData>| e.prevent_default(),
            ondrop:     on_drop,

            div { class: "column-header",
                span { class: "column-name", "{col_name}" }
                span {
                    class: if over_wip { "column-count over-wip" } else { "column-count" },
                    "{wip_label}"
                }
            }

            div { class: "column-cards",
                for task in tasks.iter().cloned() {
                    TaskCard { task, dragging, refresh }
                }

                if *adding.read() {
                    div { class: "add-task-form",
                        textarea {
                            class: "input add-task-input",
                            placeholder: "Task title…",
                            autofocus: "true",
                            value: "{new_title}",
                            oninput:   move |e| *new_title.write() = e.value(),
                            onkeydown: do_add_keydown,
                        }
                        div { class: "add-task-actions",
                            button {
                                class: "btn btn-primary",
                                onclick: do_add_onclick,
                                "Add"
                            }
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
                        span { class: "add-icon", "+" }
                        " Add task"
                    }
                }
            }
        }
    }
}

// ── Task card ─────────────────────────────────────────────────────────────────

#[component]
fn TaskCard(
    task:        Task,
    dragging:    Signal<Option<TaskId>>,
    mut refresh: Signal<u64>,
) -> Element {
    let ctx        = use_context::<AppContext>();
    let tid        = task.id.clone();
    let is_dragging = dragging.read().as_ref() == Some(&tid);

    rsx! {
        div {
            class: if is_dragging { "task-card dragging" } else { "task-card" },
            draggable: "true",
            ondragstart: move |_| *dragging.write() = Some(tid.clone()),
            ondragend:   move |_| *dragging.write() = None,

            div { class: "task-title", "{task.title}" }

            div { class: "task-meta",
                span {
                    class: "chip {task.priority.chip_class()}",
                    "{task.priority.chip_label()}"
                }
                if let Some(due) = task.due_date {
                    span { class: "task-due muted", "⏰ {due}" }
                }
            }

            if !task.tags.is_empty() {
                div { class: "task-tags",
                    for tag in task.tags.iter() {
                        span { class: "task-tag", "#{tag}" }
                    }
                }
            }

            div { class: "task-actions",
                button {
                    class: "task-action-btn",
                    title: "Archive task",
                    onclick: move |_| {
                        let c   = ctx.clone();
                        let tid2 = task.id.clone();
                        spawn(async move {
                            archive_task(c, tid2).await;
                            *refresh.write() += 1;
                        });
                    },
                    "✕"
                }
            }
        }
    }
}

// ── Priority display helpers ──────────────────────────────────────────────────

trait PriorityDisplay {
    fn chip_label(self) -> &'static str;
    fn chip_class(self) -> &'static str;
}

impl PriorityDisplay for Priority {
    fn chip_label(self) -> &'static str {
        match self {
            Self::Low      => "Low",
            Self::Medium   => "Med",
            Self::High     => "High",
            Self::Critical => "Crit",
        }
    }
    fn chip_class(self) -> &'static str {
        match self {
            Self::Low      => "priority-low",
            Self::Medium   => "priority-medium",
            Self::High     => "priority-high",
            Self::Critical => "priority-critical",
        }
    }
}

// ── New-board helpers ─────────────────────────────────────────────────────────

#[component]
fn NewBoardPrompt(mut refresh: Signal<u64>) -> Element {
    let ctx = use_context::<AppContext>();
    let mut name = use_signal(|| "My Board".to_string());

    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();

    rsx! {
        div { class: "kanban-empty",
            div { class: "kanban-empty-card",
                h2 { class: "kanban-empty-heading", "Create your first board" }
                p  { class: "muted", "Give it a name — you can rename it later." }
                div { class: "kanban-empty-form",
                    input {
                        class: "input",
                        r#type: "text",
                        value: "{name}",
                        oninput:   move |e| *name.write() = e.value(),
                        onkeydown: move |e| {
                            if e.key() == Key::Enter {
                                let n = name.read().trim().to_string();
                                if n.is_empty() { return; }
                                let c = ctx1.clone();
                                spawn(async move {
                                    create_board(c, n).await;
                                    *refresh.write() += 1;
                                });
                            }
                        },
                    }
                    button {
                        class: "btn btn-primary",
                        onclick: move |_| {
                            let n = name.read().trim().to_string();
                            if n.is_empty() { return; }
                            let c = ctx2.clone();
                            spawn(async move {
                                create_board(c, n).await;
                                *refresh.write() += 1;
                            });
                        },
                        "Create board"
                    }
                }
            }
        }
    }
}

#[component]
fn NewBoardInline(mut refresh: Signal<u64>) -> Element {
    let ctx      = use_context::<AppContext>();
    let mut open = use_signal(|| false);
    let mut name = use_signal(String::new);

    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();

    rsx! {
        if *open.read() {
            div { class: "new-board-inline",
                input {
                    class: "input new-board-input",
                    r#type: "text",
                    placeholder: "Board name…",
                    autofocus: "true",
                    value: "{name}",
                    oninput:   move |e| *name.write() = e.value(),
                    onkeydown: move |e| {
                        match e.key() {
                            Key::Enter => {
                                let n = name.read().trim().to_string();
                                if n.is_empty() { return; }
                                let c = ctx1.clone();
                                spawn(async move {
                                    create_board(c, n).await;
                                    *refresh.write() += 1;
                                });
                                *open.write() = false;
                                *name.write() = String::new();
                            }
                            Key::Escape => {
                                *open.write() = false;
                                *name.write() = String::new();
                            }
                            _ => {}
                        }
                    },
                }
                button {
                    class: "btn btn-primary",
                    onclick: move |_| {
                        let n = name.read().trim().to_string();
                        if n.is_empty() { return; }
                        let c = ctx2.clone();
                        spawn(async move {
                            create_board(c, n).await;
                            *refresh.write() += 1;
                        });
                        *open.write() = false;
                        *name.write() = String::new();
                    },
                    "Create"
                }
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| {
                        *open.write() = false;
                        *name.write() = String::new();
                    },
                    "✕"
                }
            }
        } else {
            button {
                class: "board-tab new-board-btn",
                onclick: move |_| *open.write() = true,
                "+ New board"
            }
        }
    }
}
