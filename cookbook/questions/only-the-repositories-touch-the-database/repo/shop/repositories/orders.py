from shop.db import session


def order_total(order_id: str) -> int:
    rows = session.query("select id, total from orders")
    return next((total for row_id, total in rows if row_id == order_id), 0)
