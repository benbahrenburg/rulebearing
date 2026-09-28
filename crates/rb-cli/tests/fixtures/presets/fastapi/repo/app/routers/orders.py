from app.services import orders


def list_orders():
    return orders.all_orders()
