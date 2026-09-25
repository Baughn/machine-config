"""Approvals and AskUserQuestion answers. Pure except for the futures they settle."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
import enum
from typing import Any

STOP = "🛑"


class Verdict(enum.Enum):
    ALLOW = "allow"
    DENY = "deny"


def validate_questions(tool_input: dict[str, Any]) -> list[dict[str, Any]]:
    questions = tool_input.get("questions")
    if not isinstance(questions, list) or not questions:
        raise ValueError("AskUserQuestion needs at least one question")
    for question in questions:
        if not isinstance(question, dict) or not isinstance(question.get("question"), str):
            raise ValueError("each question needs question text")
        options = question.get("options")
        if not isinstance(options, list) or not options or len(options) > 25:
            raise ValueError("each question needs 1-25 options")
    return questions


def answers(questions: list[dict[str, Any]], selections: dict[int, list[str]]) -> dict[str, Any]:
    """Map per-question selected labels back to AskUserQuestion's `answers` field."""
    result: dict[str, Any] = {}
    for index, question in enumerate(questions):
        labels = [str(o.get("label")) for o in question["options"]]
        chosen = [label for label in selections.get(index, []) if label in labels]
        if not chosen:
            raise ValueError(f"question {index + 1} has no valid answer")
        if question.get("multiSelect"):
            result[question["question"]] = chosen
        else:
            if len(chosen) != 1:
                raise ValueError(f"question {index + 1} takes exactly one answer")
            result[question["question"]] = chosen[0]
    return result


@dataclass
class Pending:
    """One outstanding approval or question, keyed by its Discord message id."""

    message_id: str
    future: asyncio.Future[Any]
    questions: list[dict[str, Any]] | None = None
    selections: dict[int, list[str]] = field(default_factory=dict)
    decided_by: str | None = None

    def select(self, index: int, labels: list[str]) -> bool:
        """Record an approver's selection. True once every question is answered."""
        assert self.questions is not None
        if not 0 <= index < len(self.questions):
            return False
        self.selections[index] = labels
        if len(self.selections) < len(self.questions):
            return False
        try:
            result = answers(self.questions, self.selections)
        except ValueError:
            return False
        if not self.future.done():
            self.future.set_result(result)
        return True

    def decide(self, verdict: Verdict) -> None:
        if not self.future.done():
            self.future.set_result(verdict)

    def cancel(self, reason: str) -> None:
        if not self.future.done():
            self.future.set_exception(Cancelled(reason))


class Cancelled(Exception):
    pass
