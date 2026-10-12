from shop.orders import status


def remind(order_id: str) -> str | None:
    return f"reminder for {order_id}" if status.is_open(order_id) else None
