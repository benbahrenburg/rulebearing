from shop.services import orders
from shop.web import formatting


def show_order(order_id: str) -> str:
    return formatting.money(orders.total(order_id))
