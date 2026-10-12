from shop.web import formatting


def total(lines: list[int]) -> int:
    return sum(lines)


def label(lines: list[int]) -> str:
    return formatting.money(total(lines))
