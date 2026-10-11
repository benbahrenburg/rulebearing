from shop.main import run


def total(n: int) -> int:
    return n if n < 10 else len(run())
