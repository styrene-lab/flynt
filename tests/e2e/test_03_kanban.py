"""Test 3: Kanban Board — UI Guide Section 4."""

import platform
import pytest

needs_webview = pytest.mark.skipif(
    platform.system() != "Linux",
    reason="DOM tests require Linux WebKit inspector (CDP)"
)


@needs_webview
class TestBoardCreation:
    def test_empty_board_shows_prompt(self, app):
        app.page.click(".nav-btn[title='Kanban']")
        app.page.wait_for_selector(".view-kanban", timeout=5000)
        prompt = app.page.query_selector(".new-board-prompt, .board-tabs")
        assert prompt is not None

    def test_create_board(self, app):
        app.page.click(".nav-btn[title='Kanban']")
        app.page.wait_for_selector(".view-kanban", timeout=5000)
        new_btn = app.page.query_selector(".new-board-prompt input, .board-tab.new")
        if new_btn:
            if new_btn.get_attribute("class") and "board-tab" in (new_btn.get_attribute("class") or ""):
                new_btn.click()
                app.page.wait_for_selector(".board-new-inline input", timeout=2000)
                app.page.fill(".board-new-inline input", "Test Board")
                app.page.keyboard.press("Enter")
            else:
                app.page.fill(".new-board-prompt input", "Test Board")
                app.page.click(".new-board-prompt .btn-primary")
        app.page.wait_for_selector(".board-tab", timeout=5000)
        tabs = app.page.query_selector_all(".board-tab")
        tab_names = [t.text_content() for t in tabs]
        assert "Test Board" in tab_names


@needs_webview
class TestColumns:
    def test_default_columns_exist(self, app):
        app.page.click(".nav-btn[title='Kanban']")
        app.page.wait_for_selector(".view-kanban", timeout=5000)
        new_btn = app.page.query_selector(".new-board-prompt input")
        if new_btn:
            app.page.fill(".new-board-prompt input", "Col Test")
            app.page.click(".new-board-prompt .btn-primary")
            app.page.wait_for_selector(".kanban-board", timeout=5000)
        columns = app.page.query_selector_all(".kanban-column-name")
        names = [c.text_content() for c in columns]
        assert "Backlog" in names
        assert "Done" in names

    def test_add_column(self, app):
        app.page.click(".nav-btn[title='Kanban']")
        app.page.wait_for_selector(".view-kanban", timeout=5000)
        new_btn = app.page.query_selector(".new-board-prompt input")
        if new_btn:
            app.page.fill(".new-board-prompt input", "Add Col Test")
            app.page.click(".new-board-prompt .btn-primary")
            app.page.wait_for_selector(".kanban-board", timeout=5000)
        app.page.click(".add-column-btn")
        app.page.wait_for_selector(".add-column-form input", timeout=2000)
        app.page.fill(".add-column-form input", "Custom Column")
        app.page.keyboard.press("Enter")
        app.page.wait_for_timeout(1000)
        columns = app.page.query_selector_all(".kanban-column-name")
        names = [c.text_content() for c in columns]
        assert "Custom Column" in names


@needs_webview
class TestTasks:
    def test_add_task(self, app):
        app.page.click(".nav-btn[title='Kanban']")
        app.page.wait_for_selector(".view-kanban", timeout=5000)
        new_btn = app.page.query_selector(".new-board-prompt input")
        if new_btn:
            app.page.fill(".new-board-prompt input", "Task Test")
            app.page.click(".new-board-prompt .btn-primary")
            app.page.wait_for_selector(".kanban-board", timeout=5000)
        app.page.click(".add-task-btn")
        app.page.wait_for_selector(".new-task-card input", timeout=2000)
        app.page.fill(".new-task-card input", "My First Task")
        app.page.click(".new-task-card .btn-primary")
        app.page.wait_for_timeout(1000)
        cards = app.page.query_selector_all(".task-title")
        titles = [c.text_content() for c in cards]
        assert "My First Task" in titles
