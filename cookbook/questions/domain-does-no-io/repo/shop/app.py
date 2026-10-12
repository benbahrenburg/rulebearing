from shop.db import session
from shop.domain import invoice
from shop.http import client


def run() -> None:
    total = invoice.total([1000, 250])
    session.save("invoice", total)
    client.post("/invoices", total)
