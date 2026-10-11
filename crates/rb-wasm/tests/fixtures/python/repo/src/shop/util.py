import importlib


def fmt(n: int) -> str:
    return f"{n:.2f}" + str(importlib.import_module("shop.models"))
