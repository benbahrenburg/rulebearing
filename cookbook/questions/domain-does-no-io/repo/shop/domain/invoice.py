from shop.db import session
from shop.domain import money


def total(lines: list[int]) -> int:
    return money.add(lines)


def total_of(invoice_id: str) -> int:
    return total(session.load_lines(invoice_id))
