from shop.domain import order


def total(order_id: str) -> int:
    return order.Order(order_id, [1000, 250]).total()
