from shop.db import pool


def query(sql: str) -> list[tuple[str, int]]:
    return pool.connection().execute(sql)
