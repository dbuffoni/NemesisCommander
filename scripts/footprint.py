#!/usr/bin/env python3
"""Keeps an eye on NemesisCommander's footprint (Linux).

  scripts/footprint.py size FILE MAX_MB
      Fails when FILE is larger than MAX_MB.

  scripts/footprint.py idle MAX_MB -- COMMAND...
      Starts COMMAND (standard input kept open), waits two seconds and
      fails when it uses more than MAX_MB of memory (resident set).

  scripts/footprint.py bench nmc
      Times a few everyday jobs of the `nmc` program on generated files
      and prints a Markdown table with their time and peak memory (which
      includes about 10 MB of this script's own). Only informative: it
      never fails on numbers.

CI runs it on the release builds (see .github/workflows/release.yml) and
writes the results to the job summary. Budgets live in that workflow:
raise one only on purpose, in the commit that needs it.
"""

import os
import shutil
import subprocess
import sys
import tempfile
import time
import zipfile

MIB = 1024 * 1024


def say(line):
    """Prints `line`, and adds it to the CI job summary when there is one."""
    print(line)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as out:
            out.write(line + "\n")


def check(name, value_mb, max_mb):
    ok = value_mb <= max_mb
    mark = "ok" if ok else "OVER BUDGET"
    say(f"| {name} | {value_mb:.1f} MB | {max_mb:g} MB | {mark} |")
    return ok


def size(path, max_mb):
    return check(f"size of {os.path.basename(path)}", os.path.getsize(path) / MIB, max_mb)


def idle(max_mb, command):
    child = subprocess.Popen(
        command, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
    )
    try:
        time.sleep(2)
        if child.poll() is not None:
            print(f"{command[0]} ended early with {child.returncode}", file=sys.stderr)
            return False
        with open(f"/proc/{child.pid}/status", encoding="utf-8") as status:
            rss_kb = next(int(l.split()[1]) for l in status if l.startswith("VmRSS:"))
    finally:
        child.kill()
        child.wait()
    name = " ".join([os.path.basename(command[0])] + command[1:])
    return check(f"idle memory of {name}", rss_kb / 1024, max_mb)


def measure(command):
    """Runs `command` and returns (seconds, peak memory in MB)."""
    # posix_spawn (not fork): the child's peak then counts only the
    # program, not a copy of this interpreter.
    quiet = [(os.POSIX_SPAWN_OPEN, 1, os.devnull, os.O_WRONLY, 0)]
    start = time.monotonic()
    pid = os.posix_spawnp(command[0], command, os.environ, file_actions=quiet)
    _, status, usage = os.wait4(pid, 0)
    seconds = time.monotonic() - start
    if os.waitstatus_to_exitcode(status) != 0:
        raise SystemExit(f"failed: {' '.join(command)}")
    return seconds, usage.ru_maxrss / 1024


def bench(nmc):
    work = tempfile.mkdtemp(prefix="nmc-bench-")
    try:
        # Made by another process, which keeps this one small: a program
        # started from here starts with this process's peak memory.
        subprocess.run([sys.executable, "-I", __file__, "make-files", work], check=True)
        many = os.path.join(work, "many")
        big = os.path.join(work, "big.bin")
        archive = os.path.join(work, "entries.zip")
        copy = os.path.join(work, "copy")
        packed = os.path.join(work, "many.zip")

        jobs = [
            ("list 20,000 files", [nmc, "ls", "-l", many]),
            ("hash 256 MB (blake3, memory-mapped)", [nmc, "hash", big]),
            ("copy 256 MB", [nmc, "cp", big, copy]),
            ("open a zip of 100,000 entries", [nmc, "ls", f"zip://{archive}!/d1"]),
            ("pack 20,000 files (zip)", [nmc, "pack", many, "-o", packed]),
        ]
        say("| job | time | peak memory |")
        say("|---|---|---|")
        for name, command in jobs:
            # The first run warms the disk cache; the second one counts.
            for _ in range(2):
                for output in (copy, packed):
                    if os.path.exists(output):
                        os.remove(output)
                seconds, peak = measure(command)
            say(f"| {name} | {seconds:.2f} s | {peak:.1f} MB |")
    finally:
        shutil.rmtree(work, ignore_errors=True)
    return True


def make_files(work):
    many = os.path.join(work, "many")
    os.mkdir(many)
    for i in range(20000):
        with open(os.path.join(many, f"file{i:05}.txt"), "w") as f:
            f.write("x")
    with open(os.path.join(work, "big.bin"), "wb") as f:
        for _ in range(256):
            f.write(os.urandom(MIB))
    with zipfile.ZipFile(os.path.join(work, "entries.zip"), "w") as z:
        for i in range(100000):
            z.writestr(f"d{i % 100}/file{i}.txt", "x")
    return True


def main(args):
    if len(args) == 3 and args[0] == "size":
        ok = size(args[1], float(args[2]))
    elif len(args) >= 4 and args[0] == "idle" and args[2] == "--":
        ok = idle(float(args[1]), args[3:])
    elif len(args) == 2 and args[0] == "make-files":
        ok = make_files(args[1])
    elif len(args) == 2 and args[0] == "bench":
        ok = bench(args[1])
    else:
        print(__doc__, file=sys.stderr)
        return 2
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
