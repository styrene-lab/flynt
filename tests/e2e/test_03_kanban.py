"""Test 3: Kanban Board — UI Guide Section 4."""

import pytest


class TestBoardCreation:
    """Board lifecycle."""

    def test_empty_board_shows_prompt(self, app):
        """With no boards, kanban view shows creation prompt."""
        app.page.click(".nav-btn[title='Kanban']")
        app.page.wait_for_selector(".view-kanban", timeout=5000)
        # Should show new board prompt or empty state
        prompt = app.page.query_selector(".new-board-prompt, .board-tabs")
        assert prompt is not None

    def test_create_board(self, app):
        """Creating a board adds it to the tab bar."""
        app.page.click(".nav-btn[title='Kanban']")
        app.page.wait_for_selector(".view-kanban", timeout=5000)

        # Find the new board input
        new_btn = app.page.query_selector(".new-board-prompt input, .board-tab.new")
        if new_btn:
            if new_btn.get_attribute("class") and "board-tab" in new_btn.get_attribute("class"):
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


class TestColumns:
    """Column management."""

    def test_default_columns_exist(self, app):
        """A new board has default columns."""
        app.page.click(".nav-btn[title='Kanban']")
        app.page.wait_for_selector(".view-kanban", timeout=5000)

        # Create a board first
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
        """Add column button creates a new column."""
        app.page.click(".nav-btn[title='Kanban']")
        app.page.wait_for_selector(".view-kanban", timeout=5000)

        # Create board
        new_btn = app.page.query_selector(".new-board-prompt input")
        if new_btn:
            app.page.fill(".new-board-prompt input", "Add Col Test")
            app.page.click(".new-board-prompt .btn-primary")
            app.page.wait_for_selector(".kanban-board", timeout=5000)

        # Click add column
        app.page.click(".add-column-btn")
        app.page.wait_for_selector(".add-column-form input", timeout=2000)
        app.page.fill(".add-column-form input", "Custom Column")
        app.page.keyboard.press("Enter")

        app.page.wait_for_timeout(1000)
        columns = app.page.query_selector_all(".kanban-column-name")
        names = [c.text_content() for c in columns]
        assert "Custom Column" in names


class TestTasks:
    """Task card operations."""

    def test_add_task(self, app):
        """Add task button creates a task card in the column."""
        app.page.click(".nav-btn[title='Kanban']")
        app.page.wait_for_selector(".view-kanban", timeout=5000)

        # Create board
        new_btn = app.page.query_selector(".new-board-prompt input")
        if new_btn:
            app.page.fill(".new-board-prompt input", "Task Test")
            app.page.click(".new-board-prompt .btn-primary")
            app.page.wait_for_selector(".kanban-board", timeout=5000)

        # Add task to first column
        app.page.click(".add-task-btn")
        app.page.wait_for_selector(".new-task-card input", timeout=2000)
        app.page.fill(".new-task-card input", "My First Task")
        app.page.click(".new-task-card .btn-primary")

        app.page.wait_for_timeout(1000)
        cards = app.page.query_selector_all(".task-title")
        titles = [c.text_content() for c in cards]
        assert "My First Task" in titles
