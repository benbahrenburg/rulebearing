from shop.db import session
from shop.repositories import orders


def show_order(order_id: str) -> str:
    return f"{order_id}: {orders.order_total(order_id)}"


def count_orders() -> int:
    return len(session.query("select id, total from orders"))
