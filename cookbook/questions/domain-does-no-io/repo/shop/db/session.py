STORE: dict[str, object] = {}


def save(key: str, value: object) -> None:
    STORE[key] = value


def load_lines(key: str) -> list[int]:
    return [1000, 250] if key else []
