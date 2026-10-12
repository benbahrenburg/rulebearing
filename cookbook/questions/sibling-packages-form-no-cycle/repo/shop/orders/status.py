def is_open(order_id: str) -> bool:
    return order_id.startswith("o-")
