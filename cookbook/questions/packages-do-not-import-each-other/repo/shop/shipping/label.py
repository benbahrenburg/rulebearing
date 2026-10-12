from shop.common import ids


def label(order_id: str) -> str:
    return f"{ids.new_id('parcel')} for {order_id}"
