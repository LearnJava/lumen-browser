# Local stand-in for the upstream `tools/wpt/utils.py` helper module, which the
# vendored tree does not carry (tools/wpt itself is not vendored — see
# tests/wpt/VENDOR.md). Only what WPT python-file handlers import is here;
# grow on demand, keep upstream-compatible signatures.
import importlib.util
from importlib.machinery import SourceFileLoader


def load_source(name, path):
    """Load a python file as a module object (upstream `tools.wpt.utils`)."""
    loader = SourceFileLoader(name, path)
    spec = importlib.util.spec_from_loader(name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module
