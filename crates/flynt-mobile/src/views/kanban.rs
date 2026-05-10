use flynt_core::{
    models::{BoardId, Priority, Task, TaskStatus},
    store::{TaskFilter, ProjectStore},
};
use dioxus::prelude::*;
use crate::bootstrap::MobileRuntime;

#[component]
pub fn KanbanView() -> Element {
    let rt = use_context::<Signal<MobileRuntime>>();
    let refresh = use_signal(|| 0u64);

    let boards = use_memo(move || {
        let _ = *refresh.read();
        rt.read().project.store.list_boards().unwrap_or_default()
    });

    let mut active_board: Signal<Option<BoardId>> = use_signal(|| None);

    // Auto-select first board
    if active_board.read().is_none() {
        if let Some(b) = boards.read().first() {
            *active_board.write() = Some(b.id.clone());
        }
    }

    let board_list = boards.read();
    if board_list.is_empty() {
        return rsx! {
            div { class: "kanban-empty-mobile",
                h2 { "No boards yet" }
                p { class: "muted", "Create a board on desktop to see it here." }
            }
        };
    }

    let active = board_list
        .iter()
        .find(|b| active_board.read().as_ref() == Some(&b.id))
        .cloned();

    rsx! {
        div { class: "kanban-mobile",
            // Board selector
            if board_list.len() > 1 {
                div { class: "board-tabs-mobile",
                    for board in board_list.iter() {
                        {
                            let bid = board.id.clone();
                            let is_active = active_board.read().as_ref() == Some(&bid);
                            rsx! {
                                button {
                                    class: if is_active { "board-tab-m active" } else { "board-tab-m" },
                                    onclick: move |_| *active_board.write() = Some(bid.clone()),
                                    "{board.name}"
                                }
                            }
                        }
                    }
                }
            }

            // Board columns
            if let Some(board) = active {
                {
                    let tasks = {
                        let _ = *refresh.read();
                        rt.read()
                            .project
                            .store
                            .list_tasks(&TaskFilter {
                                board_id: Some(board.id.clone()),
                                ..Default::default()
                            })
                            .unwrap_or_default()
                    };
                    rsx! {
                        div { class: "kanban-columns-mobile",
                            for col in board.columns.iter() {
                                {
                                    let col_tasks: Vec<&Task> = tasks
                                        .iter()
                                        .filter(|t| t.column == col.name && t.status != TaskStatus::Archived)
                                        .collect();
                                    rsx! {
                                        div { class: "kanban-col-mobile",
                                            div { class: "kanban-col-header-m",
                                                span { "{col.name}" }
                                                span { class: "kanban-count-m", "{col_tasks.len()}" }
                                            }
                                            for task in col_tasks.iter() {
                                                div { class: "task-card-mobile",
                                                    div { class: "task-priority-m {priority_class(task.priority)}" }
                                                    span { "{task.title}" }
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
        }
    }
}

fn priority_class(p: Priority) -> &'static str {
    match p {
        Priority::Low => "low",
        Priority::Medium => "medium",
        Priority::High => "high",
        Priority::Critical => "critical",
    }
}
