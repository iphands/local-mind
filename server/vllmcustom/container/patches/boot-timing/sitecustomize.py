# boot-timing: interpreter-start entry point for the startup stopwatch.
#
# Mounted at /opt/boot-timing via PYTHONPATH (see BOOT_TIMING in
# qwen3.8-flash-next/run). The interpreter imports THIS file as `sitecustomize`
# before running any code, in every process of the container -- including the
# multiprocessing-spawn'd EngineCore (spawn children re-run site with the
# inherited PYTHONPATH). That is where boot_timing installs its T0 and the
# sys.meta_path hook.
#
# The image's base python ships /usr/lib/python3.12/sitecustomize.py (Debian's
# apport-hook installer). PYTHONPATH precedes the stdlib on sys.path, so we
# SHADOW it -- therefore we must chain it explicitly, or apport handling would
# silently vanish. BOOT_TIMING=0 disables the stopwatch but keeps the chain.

import os
import sys

if os.environ.get("BOOT_TIMING", "1") != "0":
    try:
        import boot_timing

        boot_timing.init()
    except Exception as e:  # a broken timer must never break the interpreter
        print(f"[boot-timing] disabled (init failed: {e!r})", file=sys.stderr, flush=True)

try:
    _deb = "/usr/lib/python3.12/sitecustomize.py"
    if os.path.isfile(_deb) and os.path.realpath(__file__) != os.path.realpath(_deb):
        import importlib.util

        _spec = importlib.util.spec_from_file_location("_debian_sitecustomize", _deb)
        if _spec is not None and _spec.loader is not None:
            _mod = importlib.util.module_from_spec(_spec)
            _spec.loader.exec_module(_mod)
except Exception:
    pass
