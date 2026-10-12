from shop.common import ids


def issue(order_id: str, total_cents: int) -> str:
    return f"{ids.new_id('invoice')} for {order_id}: {total_cents}"
