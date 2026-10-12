from shop.domain import pricing


class Order:
    def __init__(self, order_id: str, lines: list[int]) -> None:
        self.order_id = order_id
        self.lines = lines

    def total(self) -> int:
        return pricing.total(self.lines)
