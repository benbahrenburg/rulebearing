class Connection:
    def execute(self, sql: str) -> list[tuple[str, int]]:
        return [("o-1", 1250)] if sql else []


def connection() -> Connection:
    return Connection()
