"""The code-layer fixture."""
import abc
from abc import ABC, abstractmethod
from dataclasses import dataclass

from .core import Engine
from .sub.sibling import Sibling


class Shape(ABC):
    @abstractmethod
    def area(self): ...


class Square(Shape, Sibling):
    def __init__(self, side):
        self._side = side

    @property
    def side(self):
        return self._side

    @side.setter
    def side(self, value):
        self._side = value

    @property
    def label(self):
        return "square"

    @staticmethod
    def unit():
        return Square(1)

    @classmethod
    def of(cls, side):
        return cls(side)

    def _secret(self):
        return 42

    def area(self):
        return self._side ** 2

    class Corner:
        pass


@dataclass(frozen=True)
class Point:
    x: int
    y: int


@dataclass
class Loose:
    x: int


class Meta(metaclass=abc.ABCMeta):
    pass


class _Private(Engine):
    pass


def make_square(side):
    return Square(side)


def _helper():
    return None
