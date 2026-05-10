use chrono::Utc;
use flynt_core::{
    models::{Board, BoardId, Column, Engagement, EngagementId, Priority, Task, TaskId, TaskStatus},
    store::{TaskFilter, VaultStore},
};
use dioxus::prelude::*;
use crate::bootstrap::AppContext;
use crate::state::{Route, TabState};

// ── Shared async helpers (avoid move-closure duplication) ────────────────────

async fn create_task(ctx: AppContext, board_id: BoardId, col: String, title: String, project_id: Option<uuid::Uuid>) {
    let vault = ctx.vault();
    let _ = tokio::task::spawn_blocking(move || {
        let task = Task::new(board_id, col, title);
        vault.persist_task(&task, project_id)
    })
    .await;
}

async fn move_task(ctx: AppContext, task_id: TaskId, col: String, project_id: Option<uuid::Uuid>) {
    let vault = ctx.vault();
    let _ = tokio::task::spawn_blocking(move || {
        if let Ok(Some(mut t)) = vault.store.get_task(&task_id) {
            t.column     = col;
            t.updated_at = Utc::now();
            vault.persist_task(&t, project_id)
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
            vault.persist_task(&t, project_id)
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

async fn delete_board(ctx: AppContext, board_id: BoardId) -> anyhow::Result<()> {
    let vault = ctx.vault();
    tokio::task::spawn_blocking(move || vault.store.delete_board(&board_id))
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))??;
    Ok(())
}

// ── Top-level view ────────────────────────────────────────────────────────────

#[component]
pub fn KanbanView() -> Element {
    let ctx = use_context::<AppContext>();
    let mut refresh = use_signal(|| 0_u64);

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

    let mut confirm_delete: Signal<Option<BoardId>> = use_signal(|| None);

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

                        // Delete active board — right-aligned
                        if let Some(active_id) = active_board.read().clone() {
                            {
                                let is_confirming = confirm_delete.read().as_ref() == Some(&active_id);
                                rsx! {
                                    div { class: "board-delete-zone",
                                        if is_confirming {
                                            span { class: "board-delete-confirm-label", "Delete this board and its tasks? This cannot be undone." }
                                            button {
                                                class: "btn btn-danger btn-sm",
                                                onclick: move |_| {
                                                    let c = ctx.clone();
                                                    let bid = active_id.clone();
                                                    spawn(async move {
                                                        if delete_board(c, bid).await.is_ok() {
                                                            *active_board.write() = None;
                                                            *refresh.write() += 1;
                                                        }
                                                    });
                                                    *confirm_delete.write() = None;
                                                },
                                                "Confirm"
                                            }
                                            button {
                                                class: "btn btn-ghost btn-sm",
                                                onclick: move |_| *confirm_delete.write() = None,
                                                "Cancel"
                                            }
                                        } else {
                                            button {
                                                class: "board-delete-btn",
                                                title: "Delete this board",
                                                onclick: move |_| *confirm_delete.write() = Some(active_id.clone()),
                                                "\u{2715}"
                                            }
                                        }
                                    }
                                }
                            }
                        }
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

/// Filter slice applied above the kanban columns. `All` is the default
/// (non-archived everywhere); the others are the operator's view-modes
/// for working with sentry-managed tasks. `Archived` is opt-in only —
/// archived tasks are otherwise hidden everywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskFilterKind {
    All,
    Actionable,
    LifecycleLinked,
    Manual,
    Archived,
}

impl TaskFilterKind {
    fn label(&self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Actionable => "Actionable",
            Self::LifecycleLinked => "Lifecycle",
            Self::Manual => "Manual",
            Self::Archived => "Archived",
        }
    }
    fn title(&self) -> &'static str {
        match self {
            Self::All => "All non-archived tasks",
            Self::Actionable => "Tasks that sentry would pick up next: column=Scheduled, status=Todo",
            Self::LifecycleLinked => "Tasks linked to a design node or openspec change",
            Self::Manual => "Hand-tracked tasks (no sentry triggers or execution metadata)",
            Self::Archived => "Archived tasks (otherwise hidden everywhere)",
        }
    }
    /// Apply the filter to a single task. Caller still applies the
    /// per-column name match separately.
    fn matches(&self, task: &Task) -> bool {
        match self {
            Self::All => task.status != TaskStatus::Archived,
            Self::Actionable => {
                task.column == "Scheduled"
                    && task.status == TaskStatus::Todo
            }
            Self::LifecycleLinked => {
                task.status != TaskStatus::Archived
                    && (task.design_node_id.is_some() || task.openspec_change.is_some())
            }
            Self::Manual => {
                task.status != TaskStatus::Archived && !task.is_sentry_managed()
            }
            Self::Archived => task.status == TaskStatus::Archived,
        }
    }
}

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

    // Engagement records — populates the scope dropdown above the
    // columns and the engagement picker in the inline task editor.
    // Refreshes alongside tasks; small enough that polling per refresh
    // is fine.
    let engagements = use_resource(move || {
        let _ = refresh();
        let vault = ctx.vault();
        async move {
            tokio::task::spawn_blocking(move || vault.store.list_engagements().unwrap_or_default())
                .await
                .unwrap_or_default()
        }
    });

    let dragging: Signal<Option<TaskId>> = use_signal(|| None);
    // Active filter pill — All by default. Persists for the board view's
    // lifetime; resets when switching boards (KanbanBoard re-mounts).
    let mut active_filter = use_signal(|| TaskFilterKind::All);
    // Engagement scope — None = all engagements (and unscoped tasks).
    // Layered with active_filter as an intersection, applied per-column.
    let mut active_engagement: Signal<Option<EngagementId>> = use_signal(|| None);

    let mut adding_col = use_signal(|| false);
    let mut new_col_name = use_signal(String::new);

    let ctx_col = ctx.clone();
    let board_for_add = board.clone();

    rsx! {
        div { class: "kanban-board-shell",
            div { class: "kanban-filter-pills",
                for kind in [
                    TaskFilterKind::All,
                    TaskFilterKind::Actionable,
                    TaskFilterKind::LifecycleLinked,
                    TaskFilterKind::Manual,
                    TaskFilterKind::Archived,
                ] {
                    {
                        let is_active = *active_filter.read() == kind;
                        rsx! {
                            button {
                                class: if is_active { "kanban-filter-pill active" } else { "kanban-filter-pill" },
                                title: kind.title(),
                                onclick: move |_| *active_filter.write() = kind,
                                "{kind.label()}"
                            }
                        }
                    }
                }

                // Engagement scope selector — sits with the filter pills
                // because operators reach for it for the same reason
                // (narrow what's on screen). Hidden when no engagements
                // exist so empty vaults don't show a useless dropdown.
                {
                    let engs = engagements.read();
                    let list: Vec<Engagement> = engs.clone().unwrap_or_default();
                    (!list.is_empty()).then(|| {
                        let active_str = active_engagement.read()
                            .as_ref()
                            .map(|e| e.0.to_string())
                            .unwrap_or_default();
                        rsx! {
                            select {
                                class: "kanban-engagement-select",
                                title: "Scope to one engagement (or All).",
                                onchange: move |e| {
                                    let v = e.value();
                                    *active_engagement.write() = if v.is_empty() {
                                        None
                                    } else {
                                        uuid::Uuid::parse_str(&v).ok().map(EngagementId)
                                    };
                                },
                                // `selected` on each option is the canonical
                                // way to pre-select; `value` on <select> isn't
                                // honored at hydration time by the WebView,
                                // so the dropdown would silently default to
                                // the first option on remount.
                                option { value: "", selected: active_str.is_empty(), "All engagements" }
                                for eng in list.iter() {
                                    {
                                        let id_str = eng.id.0.to_string();
                                        let is_sel = id_str == active_str;
                                        rsx! {
                                            option {
                                                value: "{id_str}",
                                                selected: is_sel,
                                                "{eng.name}"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    })
                }
            }
            div { class: "kanban-board",
                {
                    // Hoist (id_string, name) pairs once per render —
                    // every column + card threads the same Vec down,
                    // so rebuilding it inside the for-loop was N×
                    // wasteful. `loaded` is None during the first
                    // engagements fetch; downstream UI uses that to
                    // suppress placeholder chips while data flows in.
                    let engs_view = engagements.read();
                    let loaded = engs_view.is_some();
                    let engagement_options: Vec<(String, String)> = engs_view
                        .clone()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|e| (e.id.0.to_string(), e.name))
                        .collect();
                    drop(engs_view);
                    rsx! {
                        match &*tasks.read() {
                            None => rsx! { div { class: "kanban-loading muted", "Loading tasks…" } },
                            Some(all) => rsx! {
                            for col in board.columns.iter().cloned() {
                                {
                                    let filter = *active_filter.read();
                                    let scope = active_engagement.read().clone();
                                    let col_tasks: Vec<Task> = all.iter()
                                        .filter(|t| {
                                            if t.column != col.name { return false; }
                                            if !filter.matches(t) { return false; }
                                            // Engagement scope: when set, only
                                            // matching tasks pass. Unscoped tasks
                                            // are excluded — operator picked an
                                            // engagement explicitly.
                                            match &scope {
                                                Some(eid) => t.engagement_id.as_ref() == Some(eid),
                                                None => true,
                                            }
                                        })
                                        .cloned()
                                        .collect();
                                    let col_empty = col_tasks.is_empty();
                                    let board_for_remove = board.clone();
                                    let col_name_remove = col.name.clone();
                                    let col_name_rename = col.name.clone();
                                    let engagement_options = engagement_options.clone();
                                    rsx! {
                                        KanbanColumn {
                                            board_id: board.id.clone(),
                                            project_id,
                                            column: col,
                                            tasks: col_tasks,
                                            dragging,
                                            refresh,
                                            can_remove: col_empty,
                                            engagement_options,
                                            engagements_loaded: loaded,
                                    on_remove: move |_| {
                                        let c = ctx.clone();
                                        let mut b = board_for_remove.clone();
                                        let name = col_name_remove.clone();
                                        spawn(async move {
                                            let vault = c.vault();
                                            b.columns.retain(|c| c.name != name);
                                            let _ = tokio::task::spawn_blocking(move || {
                                                vault.store.save_board(&b)
                                            }).await;
                                            *refresh.write() += 1;
                                        });
                                    },
                                    on_rename: {
                                        let board_for_rename = board.clone();
                                        move |new_name: String| {
                                            let c = ctx.clone();
                                            let mut b = board_for_rename.clone();
                                            let old_name = col_name_rename.clone();
                                            spawn(async move {
                                                let vault = c.vault();
                                                // Rename column
                                                if let Some(col) = b.columns.iter_mut().find(|c| c.name == old_name) {
                                                    col.name = new_name.clone();
                                                }
                                                // Update tasks in this column
                                                let _ = tokio::task::spawn_blocking(move || {
                                                    vault.store.save_board(&b)?;
                                                    let tasks = vault.store.list_tasks(&flynt_core::store::TaskFilter {
                                                        board_id: Some(b.id.clone()),
                                                        column: Some(old_name),
                                                        ..Default::default()
                                                    })?;
                                                    for mut t in tasks {
                                                        t.column = new_name.clone();
                                                        // project_id is the board's; pass through
                                                        // so project-backed boards keep writing to
                                                        // git-backing.
                                                        vault.persist_task(&t, b.project_id)?;
                                                    }
                                                    Ok::<_, anyhow::Error>(())
                                                }).await;
                                                *refresh.write() += 1;
                                            });
                                        }
                                    },
                                }
                            }
                        }
                    }

                    // Add column button
                    if *adding_col.read() {
                        div { class: "kanban-column add-column-form",
                            input {
                                class: "input input-sm",
                                autofocus: true,
                                value: "{new_col_name}",
                                placeholder: "Column name",
                                oninput: move |e| *new_col_name.write() = e.value(),
                                onkeydown: move |e| {
                                    match e.key() {
                                        Key::Enter => {
                                            let name = new_col_name.read().trim().to_string();
                                            if name.is_empty() { return; }
                                            let c = ctx_col.clone();
                                            let mut b = board_for_add.clone();
                                            b.columns.push(Column { name, wip_limit: None });
                                            spawn(async move {
                                                let vault = c.vault();
                                                let _ = tokio::task::spawn_blocking(move || {
                                                    vault.store.save_board(&b)
                                                }).await;
                                                *refresh.write() += 1;
                                            });
                                            *new_col_name.write() = String::new();
                                            *adding_col.write() = false;
                                        }
                                        Key::Escape => {
                                            *new_col_name.write() = String::new();
                                            *adding_col.write() = false;
                                        }
                                        _ => {}
                                    }
                                },
                            }
                        }
                    } else {
                        button {
                            class: "add-column-btn",
                            onclick: move |_| *adding_col.write() = true,
                            "+ Add column"
                        }
                    }
                },     // close Some(all) arm
                }      // close match arms
            }          // close outer rsx!
                }      // close the let-binding block
            }          // close div kanban-board
        }              // close div kanban-board-shell
    }                  // close rsx!
}                      // close fn KanbanBoard

// ── Column ────────────────────────────────────────────────────────────────────

#[component]
fn KanbanColumn(
    board_id:   BoardId,
    project_id: Option<uuid::Uuid>,
    column:     Column,
    tasks:      Vec<Task>,
    dragging:   Signal<Option<TaskId>>,
    mut refresh: Signal<u64>,
    can_remove: bool,
    on_remove:  EventHandler<()>,
    on_rename:  EventHandler<String>,
    /// (id_string, name) pairs for the inline editor's engagement
    /// dropdown. Plain strings rather than `Vec<Engagement>` so the
    /// component prop type stays PartialEq (Engagement isn't —
    /// ForgeEndpoint upstream doesn't impl it).
    engagement_options: Vec<(String, String)>,
    /// True once the engagements list has loaded at least once. Cards
    /// suppress the engagement chip while this is false to avoid the
    /// "engagement" placeholder flashing on first render.
    engagements_loaded: bool,
) -> Element {
    let ctx         = use_context::<AppContext>();
    let col_name    = column.name.clone();
    let mut editing_name = use_signal(|| false);
    let mut edit_value = use_signal(|| column.name.clone());
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
                if *editing_name.read() {
                    input {
                        class: "kanban-column-name-input",
                        autofocus: true,
                        value: "{edit_value}",
                        oninput: move |e| *edit_value.write() = e.value(),
                        onkeydown: move |e| {
                            match e.key() {
                                Key::Enter => {
                                    let new_name = edit_value.read().trim().to_string();
                                    if !new_name.is_empty() && new_name != col_name {
                                        on_rename.call(new_name);
                                    }
                                    *editing_name.write() = false;
                                }
                                Key::Escape => {
                                    *edit_value.write() = col_name.clone();
                                    *editing_name.write() = false;
                                }
                                _ => {}
                            }
                        },
                    }
                } else {
                    span {
                        class: "kanban-column-name",
                        ondoubleclick: move |_| *editing_name.write() = true,
                        "{col_name}"
                    }
                }
                span { class: if over_wip { "kanban-wip over" } else { "kanban-wip" }, "{wip_label}" }
                if can_remove {
                    button {
                        class: "kanban-column-remove",
                        title: "Remove empty column",
                        onclick: move |_| on_remove.call(()),
                        "\u{2715}"
                    }
                }
            }

            div { class: "kanban-column-body",
                for task in tasks.iter().cloned() {
                    TaskCard {
                        task,
                        project_id,
                        dragging,
                        refresh,
                        engagement_options: engagement_options.clone(),
                        engagements_loaded,
                    }
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
fn TaskCard(
    task: Task,
    project_id: Option<uuid::Uuid>,
    dragging: Signal<Option<TaskId>>,
    mut refresh: Signal<u64>,
    /// (id_string, name) pairs for the engagement picker. Empty when
    /// no engagements exist; the Engagement subsection still renders
    /// so the operator sees a hint to create one via the agent.
    engagement_options: Vec<(String, String)>,
    /// True once the parent's engagements list has loaded at least
    /// once. Suppresses the engagement chip on first render so we
    /// don't flash "engagement" placeholder text while waiting for
    /// the list to arrive.
    engagements_loaded: bool,
) -> Element {
    let ctx = use_context::<AppContext>();
    let mut tab_state = use_context::<Signal<TabState>>();
    let mut active_route = use_context::<Signal<Route>>();
    let mut open = use_signal(|| false);
    // The inline editor is intentionally narrow: title rename, priority
    // quick-toggle, engagement quick-toggle. Description, sentry triggers,
    // execution config, lifecycle linkage all live in the task's .md
    // file — operator opens the file via "Open in editor" to edit those.
    let mut inline_title = use_signal(|| task.title.clone());
    let mut inline_priority = use_signal(|| task.priority);
    let mut inline_engagement = use_signal(|| {
        task.engagement_id.as_ref().map(|e| e.0.to_string()).unwrap_or_default()
    });

    let tid_drag = task.id.clone();
    let tid_archive = task.id.clone();
    let tid_open = task.id.clone();
    let ctx_archive = ctx.clone();
    let ctx_open = ctx.clone();
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
                button {
                    class: "task-menu-btn",
                    title: if *open.read() { "Close details" } else { "Open details" },
                    onclick: move |_| {
                        let is_open = *open.read();
                        *open.write() = !is_open;
                    },
                    if *open.read() { "−" } else { "+" }
                }
            }
            div { class: "task-card-meta",
                if !task.tags.is_empty() {
                    div { class: "task-tags-inline",
                        for tag in task.tags.iter() {
                            span { class: "task-tag", "{tag}" }
                        }
                    }
                }
                // ── Sentry-aware chips ────────────────────────────────────
                // Predicate-driven, not flag-driven. Cards self-correct as
                // fields populate. Each chip is small + low-contrast until
                // hover; full content goes in the title attr for tooltip.
                {
                    let has_cron = task.cron_trigger().is_some();
                    let has_webhook = task.webhook_trigger().is_some();
                    let has_model = task.execution.as_ref().and_then(|e| e.model.as_deref()).is_some();
                    let has_design = task.design_node_id.is_some();
                    let has_spec = task.openspec_change.is_some();
                    // Engagement chip resolves the engagement_id to a
                    // human-readable name via the same options list the
                    // dropdown uses; falls back to "engagement" if the
                    // record is missing (deleted out of band). Suppressed
                    // entirely until engagements_loaded so we don't show
                    // the placeholder during the first-render fetch.
                    let engagement_label = if engagements_loaded {
                        task.engagement_id.as_ref().map(|eid| {
                            let id_str = eid.0.to_string();
                            engagement_options.iter()
                                .find(|(id, _)| *id == id_str)
                                .map(|(_, name)| name.clone())
                                .unwrap_or_else(|| "engagement".to_string())
                        })
                    } else {
                        None
                    };
                    let has_engagement = engagement_label.is_some();
                    let any = has_cron || has_webhook || has_model || has_design || has_spec || has_engagement;
                    any.then(|| {
                        let cron_text = task.cron_trigger().map(String::from);
                        let webhook_text = task.webhook_trigger().map(String::from);
                        let model_text = task.execution.as_ref()
                            .and_then(|e| e.model.as_deref())
                            .map(short_model_label);
                        let spec_text = task.openspec_change.clone();
                        rsx! {
                            div { class: "task-card-chips",
                                if let Some(name) = engagement_label {
                                    span {
                                        class: "task-chip task-chip-engagement",
                                        title: "engagement: {name}",
                                        "◆ {name}"
                                    }
                                }
                                if let Some(cron) = cron_text {
                                    span {
                                        class: "task-chip task-chip-trigger",
                                        title: "cron trigger: {cron}",
                                        "cron"
                                    }
                                }
                                if let Some(webhook) = webhook_text {
                                    span {
                                        class: "task-chip task-chip-trigger",
                                        title: "webhook trigger: {webhook}",
                                        "webhook"
                                    }
                                }
                                if let Some(model) = model_text {
                                    span {
                                        class: "task-chip task-chip-model",
                                        title: "execution.model",
                                        "{model}"
                                    }
                                }
                                if has_design {
                                    span {
                                        class: "task-chip task-chip-design",
                                        title: "linked to a design tree node",
                                        "→ design"
                                    }
                                }
                                if let Some(spec) = spec_text {
                                    span {
                                        class: "task-chip task-chip-spec",
                                        title: "openspec change: {spec}",
                                        "↪ {spec}"
                                    }
                                }
                            }
                        }
                    })
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

                    div { class: "task-detail-row",
                        label { class: "field",
                            span { "Priority" }
                            {
                                let cur = priority_to_str(*inline_priority.read());
                                rsx! {
                                    select {
                                        onchange: move |e| {
                                            *inline_priority.write() = str_to_priority(&e.value());
                                        },
                                        option { value: "low",      selected: cur == "low",      "Low" }
                                        option { value: "medium",   selected: cur == "medium",   "Medium" }
                                        option { value: "high",     selected: cur == "high",     "High" }
                                        option { value: "critical", selected: cur == "critical", "Critical" }
                                    }
                                }
                            }
                        }

                        if !engagement_options.is_empty() {
                            label { class: "field",
                                span { "Engagement" }
                                {
                                    let cur = inline_engagement.read().clone();
                                    rsx! {
                                        select {
                                            onchange: move |e| *inline_engagement.write() = e.value(),
                                            option { value: "", selected: cur.is_empty(), "(none)" }
                                            for (id, name) in engagement_options.iter() {
                                                {
                                                    let id_str = id.clone();
                                                    let is_sel = id_str == cur;
                                                    rsx! {
                                                        option {
                                                            value: "{id_str}",
                                                            selected: is_sel,
                                                            "{name}"
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    p { class: "muted task-detail-hint",
                        "Description, sentry triggers, execution config, and lifecycle "
                        "metadata live in the task's note. Open in editor to access them."
                    }


                    div { class: "row gap-2",
                        // Save: title + priority + engagement only.
                        // Everything else is in the .md file — open it
                        // to edit description, sentry config, etc.
                        button {
                            class: "btn btn-primary",
                            onclick: move |_| {
                                let c = ctx.clone();
                                let task_id = task.id.clone();
                                let new_title = inline_title.read().trim().to_string();
                                let new_priority = *inline_priority.read();
                                let engagement_s = inline_engagement.read().trim().to_string();
                                spawn(async move {
                                    let vault = c.vault();
                                    let _ = tokio::task::spawn_blocking(move || {
                                        if let Ok(Some(mut t)) = vault.store.get_task(&task_id) {
                                            t.title      = new_title;
                                            t.priority   = new_priority;
                                            t.engagement_id = if engagement_s.is_empty() {
                                                None
                                            } else {
                                                uuid::Uuid::parse_str(&engagement_s).ok().map(EngagementId)
                                            };
                                            t.updated_at = Utc::now();
                                            vault.persist_task(&t, project_id)
                                        } else {
                                            Ok(())
                                        }
                                    }).await;
                                    *refresh.write() += 1;
                                });
                            },
                            "Save"
                        }

                        // Open the task's .md file in the notes view.
                        // Looks the document up by file path (recorded
                        // in sqlite by persist_task), opens a tab, and
                        // switches the active route. Routes silently if
                        // the file isn't indexed yet — operator can
                        // reindex via Settings.
                        button {
                            class: "btn",
                            onclick: move |_| {
                                let c = ctx_open.clone();
                                let task_id = tid_open.clone();
                                spawn(async move {
                                    let vault = c.vault();
                                    let lookup = tokio::task::spawn_blocking(move || -> Option<(flynt_core::models::DocumentId, String)> {
                                        let path = vault.store.task_file_path(&task_id).ok().flatten()?;
                                        let doc = vault.store.get_document_by_path(std::path::Path::new(&path)).ok().flatten()?;
                                        Some((doc.id, doc.title))
                                    }).await.ok().flatten();
                                    if let Some((id, title)) = lookup {
                                        tab_state.write().open(id, title);
                                        *active_route.write() = Route::Notes;
                                    } else {
                                        tracing::warn!("could not resolve task file to a document — try reindexing");
                                    }
                                });
                            },
                            "Open in editor"
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_model_strips_provider_prefix() {
        assert_eq!(short_model_label("anthropic:claude-sonnet-4-6"), "sonnet-4");
        assert_eq!(short_model_label("anthropic:claude-opus-4-7"), "opus-4");
        assert_eq!(short_model_label("anthropic:claude-haiku-4-5"), "haiku-4");
    }

    #[test]
    fn short_model_passes_through_unknown_shapes() {
        // No `claude-` prefix → return the bare part as-is.
        assert_eq!(short_model_label("openai:gpt-5-turbo"), "gpt-5-turbo");
        assert_eq!(short_model_label("ollama:qwen2-72b"), "qwen2-72b");
        assert_eq!(short_model_label("custom-model"), "custom-model");
    }

    #[test]
    fn short_model_handles_no_prefix() {
        // Bare model name, no provider colon.
        assert_eq!(short_model_label("claude-sonnet-4-6"), "sonnet-4");
    }
}

/// Short display label for an execution.model string. Strips the provider
/// prefix (`anthropic:`, `openai:`, `ollama:`) and abbreviates known long
/// model names. Card chips only have ~6-10 chars of room before wrap.
pub(crate) fn short_model_label(model: &str) -> String {
    let bare = model.split(':').last().unwrap_or(model);
    if let Some(rest) = bare.strip_prefix("claude-") {
        // claude-sonnet-4-6 → sonnet-4
        // claude-opus-4-7   → opus-4
        // claude-haiku-4-5  → haiku-4
        let parts: Vec<&str> = rest.split('-').collect();
        if parts.len() >= 2 {
            return format!("{}-{}", parts[0], parts[1]);
        }
    }
    bare.to_string()
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
                Err(e) => *error_msg.write() = Some(format!("Could not create board — {e}")),
            }
        });
        *name.write() = String::new();
    };

    rsx! {
        div { class: "new-board-prompt",
            h2 { class: "view-heading", "Create your first task board" }
            p { class: "muted", "Boards organize tasks into Backlog → Scheduled → Running → Done columns. Tasks can be hand-tracked or executed autonomously by the agent — add a cron, webhook, or execution block to a card and Sentry picks it up." }
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
                                    Err(e) => *error_msg.write() = Some(format!("Could not create board — {e}")),
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
                p { class: "text-error", "{err}" }
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
