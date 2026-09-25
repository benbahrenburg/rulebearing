"""Dynamic imports: literal importlib.import_module and __import__ calls."""
import importlib

import app.plugins


def main():
    importlib.import_module("app.plugins.greet")
    __import__("tomllib")
    importlib.import_module(".util", package="app")
    name = "app.util"
    importlib.import_module(name)
