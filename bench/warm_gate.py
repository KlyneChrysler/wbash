#!/usr/bin/env python3
import os, subprocess, sys, tempfile
from pathlib import Path

def main():
    files = int(os.environ.get("WBASH_PERF_FILES","10000"))
    # run ignored warm perf test in release context? use debug for speed of compile in CI dev build not available; CI has release built separately but tests run in debug.
    # We'll run debug test with smaller count by env; test asserts thresholds.
    cmd = ["cargo","test","-q","-p","wbash-tools","--test","warm_perf","--","--ignored","--nocapture"]
    env = os.environ.copy()
    env["WBASH_PERF_FILES"] = str(files)
    p = subprocess.run(cmd, env=env)
    return p.returncode

if __name__ == "__main__":
    raise SystemExit(main())
