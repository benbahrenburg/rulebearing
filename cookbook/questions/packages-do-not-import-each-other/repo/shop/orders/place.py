from shop.billing import invoice
from shop.common import ids


def place(total_cents: int) -> str:
    order_id = ids.new_id("order")
    invoice.issue(order_id, total_cents)
    return order_id
