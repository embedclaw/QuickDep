from helpers import format_name as helper


class BaseGreeter:
    def normalize(self, name):
        return name.strip()


class Greeter(BaseGreeter):
    def greet(self, name):
        return format_name(name)


def format_name(value):
    return value.strip()
