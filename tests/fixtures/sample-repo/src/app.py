import functools


# Registered by the router at import time.
# This comment must not be read as an attribute.
@app.route("/health")
def health():
    return "ok"


# Nothing decorates the function below.
def untouched():
    return None
