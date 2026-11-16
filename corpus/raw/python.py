from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path


@dataclass
class Record:
    name: str
    value: float
    tags: list[str] = field(default_factory=list)

    def score(self) -> float:
        bonus = sum(len(tag) for tag in self.tags) * 0.01
        return self.value + bonus


def load(path: Path) -> list[Record]:
    data = json.loads(path.read_text())
    records = []
    for row in data.get("records", []):
        try:
            records.append(Record(name=row["name"], value=float(row["value"]), tags=row.get("tags", [])))
        except (KeyError, TypeError, ValueError) as exc:
            print(f"skipping invalid row: {exc}")
    return records


def top(records: list[Record], limit: int = 10) -> list[Record]:
    return sorted(records, key=lambda r: r.score(), reverse=True)[:limit]
