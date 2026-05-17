"""Top-level package for Captain.

Provides a ``__version__`` attribute derived from the ``VERSION`` file if present.
"""

import importlib.metadata

try:
    __version__ = importlib.metadata.version(__name__)
except importlib.metadata.PackageNotFoundError:
    # Fallback: try to read a VERSION file
    try:
        from pathlib import Path
        __version__ = Path(__file__).with_name('VERSION').read_text().strip()
    except Exception:
        __version__ = '0.0.0'