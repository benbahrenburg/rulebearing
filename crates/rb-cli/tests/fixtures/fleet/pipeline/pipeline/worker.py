import json

from pipeline import jobs


def handle(message: str) -> None:
    jobs.run(json.loads(message))
