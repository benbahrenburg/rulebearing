from shop.billing import invoice
from shop.catalog import prices


def place(sku: str) -> str:
    return invoice.issue("o-1", prices.price_of(sku))
