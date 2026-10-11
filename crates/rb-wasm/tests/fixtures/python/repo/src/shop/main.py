import os
from typing import TYPE_CHECKING

import requests

from shop import models
from . import util

if TYPE_CHECKING:
    from shop.ui import view


def run() -> str:
    return util.fmt(models.total(len(os.sep))) + str(requests)
