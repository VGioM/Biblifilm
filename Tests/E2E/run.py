#!/usr/bin/env python3
"""End-to-end tests: the real app, a real VLC, real files, real quits.

Run through ./Tests/E2E/run.sh, which builds the debug app first. Every
scenario launches Biblifilm against a throwaway data folder and a separate
bundle id, so the real watch history and settings are never touched. The app
window shows up on screen while this runs; leave it be.

    ./Tests/E2E/run.sh                 # everything
    ./Tests/E2E/run.sh quit_menu next  # only the named scenarios
"""
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
APP = os.path.join(ROOT, ".build", "e2e", "BiblifilmE2E.app")
BINARY = os.path.join(APP, "Contents", "MacOS", "Biblifilm")
BUNDLE_ID = "com.biblifilm.e2e"
SAMPLE_SECONDS = 40

WORK = tempfile.mkdtemp(prefix="biblifilm-e2e-")
MEDIA = os.path.join(WORK, "media")
EP1 = os.path.join(MEDIA, "Testshow.S01E01.mp4")
EP2 = os.path.join(MEDIA, "Testshow.S01E02.mp4")
FILM = os.path.join(MEDIA, "Testfilm.2001.mp4")


# ---------------------------------------------------------------- plumbing

class Failure(Exception):
    pass


def check(condition, message):
    if not condition:
        raise Failure(message)


def make_samples():
    os.makedirs(MEDIA, exist_ok=True)
    maker = os.path.join(ROOT, "Tests", "E2E", "make-sample.swift")
    first = EP1
    subprocess.run(["swift", "-suppress-warnings", maker, first, str(SAMPLE_SECONDS)],
                   check=True, cwd=WORK)
    for other in (EP2, FILM):
        shutil.copy(first, other)


class App:
    """One launch of the app, against its own data folder."""

    def __init__(self, support, play):
        self.support = support
        os.makedirs(support, exist_ok=True)
        # A state left by the previous launch would answer the first wait.
        try:
            os.remove(os.path.join(support, "e2e-state.json"))
        except OSError:
            pass
        env = dict(os.environ,
                   BIBLIFILM_SUPPORT_DIR=support,
                   BIBLIFILM_E2E_PLAY="|".join(play))
        self.log = open(os.path.join(support, "stdout.log"), "ab")
        self.proc = subprocess.Popen([BINARY], env=env, stdout=self.log,
                                     stderr=subprocess.STDOUT)

    def state(self):
        try:
            with open(os.path.join(self.support, "e2e-state.json")) as f:
                return json.load(f)
        except (OSError, ValueError):
            return {}

    def wait_for(self, predicate, what, timeout=30):
        deadline = time.time() + timeout
        while time.time() < deadline:
            if self.proc.poll() is not None:
                raise Failure(f"app exited ({self.proc.returncode}) while waiting for {what}")
            state = self.state()
            if state and predicate(state):
                return state
            time.sleep(0.25)
        raise Failure(f"timed out waiting for {what}; last state {self.state()}")

    def wait_until_time(self, seconds, item=None):
        return self.wait_for(
            lambda s: s.get("time", 0) >= seconds and s.get("isPlaying")
                      and (item is None or s.get("current") == item),
            f"playback to reach {seconds}s" + (f" in {os.path.basename(item)}" if item else ""))

    def send(self, command):
        script = ('ObjC.import("Foundation");'
                  '$.NSDistributedNotificationCenter.defaultCenter'
                  '.postNotificationNameObjectUserInfoDeliverImmediately('
                  f'"com.biblifilm.e2e.command", {json.dumps(command)}, $(), true)')
        subprocess.run(["osascript", "-l", "JavaScript", "-e", script],
                       check=True, capture_output=True)

    def wait_exit(self, timeout=15):
        try:
            self.proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            self.kill()
            raise Failure("app did not quit")
        finally:
            self.log.close()

    def kill(self):
        if self.proc.poll() is None:
            self.proc.kill()
            self.proc.wait()

    def records(self):
        path = os.path.join(self.support, "watch-state.json")
        try:
            with open(path) as f:
                return json.load(f)
        except OSError:
            return {}


def resumable(record):
    """Mirror of WatchRecord.isResumable."""
    if not record or record.get("duration", 0) <= 0:
        return False
    progress = record["position"] / record["duration"]
    return record["position"] > 5 and progress < 0.92


# --------------------------------------------------------------- scenarios

def leave_and_check(name, leave, slack=0.75):
    """Play to ~8s, leave the way `leave` says, and expect the exact spot kept.

    The app also saves every couple of seconds while playing, so "something
    was saved" proves nothing; what is checked is that the position recorded
    is the one at the moment of leaving, give or take `slack` seconds.
    """
    support = os.path.join(WORK, name)
    app = App(support, [EP1])
    try:
        app.wait_until_time(8)
        left_at = leave(app)
    finally:
        app.kill()
    record = app.records().get(EP1)
    check(record is not None, f"nothing recorded for the episode: {app.records()}")
    check(record["position"] >= left_at - slack,
          f"saved {record['position']:.1f}s but left at {left_at:.1f}s")
    check(resumable(record), f"record not resumable: {record}")
    return support


def quit_menu():
    """⌘Q: the Quit menu item is NSApp.terminate(_:)."""
    def leave(app):
        app.wait_until_time(9.6)
        left_at = app.state()["time"]
        app.send("terminate")
        app.wait_exit()
        return left_at
    leave_and_check("quit_menu", leave)


def quit_keyboard():
    """A real ⌘Q keystroke, through the menu's key equivalent."""
    def leave(app):
        app.wait_until_time(9.6)
        left_at = app.state()["time"]
        app.send("cmdq")
        app.wait_exit()
        return left_at
    leave_and_check("quit_keyboard", leave)


def quit_fullscreen():
    """⌘Q while the film is full screen."""
    def leave(app):
        app.send("fullscreen")
        time.sleep(3)
        state = app.wait_until_time(app.state()["time"] + 1)
        app.send("cmdq")
        app.wait_exit()
        return state["time"]
    leave_and_check("quit_fullscreen", leave)


def quit_paused():
    """⌘Q after pausing."""
    def leave(app):
        app.send("pause")
        state = app.wait_for(lambda s: not s.get("isPlaying"), "pause")
        time.sleep(3)   # past the periodic save, which skips a still clock
        app.send("terminate")
        app.wait_exit()
        return state["time"]
    leave_and_check("quit_paused", leave)


def quit_appleevent():
    """Quit from the Dock, a logout or a shutdown: the 'quit' Apple Event."""
    def leave(app):
        app.wait_until_time(9.6)
        left_at = app.state()["time"]
        result = subprocess.run(
            ["osascript", "-e", f'tell application id "{BUNDLE_ID}" to quit'],
            capture_output=True, timeout=20)
        check(result.returncode == 0,
              f"osascript refused: {result.stderr.decode().strip()}")
        app.wait_exit()
        return left_at
    leave_and_check("quit_appleevent", leave)


def sigterm():
    """`kill`, or anything else that sends SIGTERM."""
    def leave(app):
        app.wait_until_time(9.6)
        left_at = app.state()["time"]
        app.proc.send_signal(signal.SIGTERM)
        app.wait_exit()
        return left_at
    leave_and_check("sigterm", leave)


def force_quit():
    """Force quit (SIGKILL): nothing can run, but at most a few seconds go."""
    def leave(app):
        app.wait_until_time(12)
        left_at = app.state()["time"]
        app.proc.send_signal(signal.SIGKILL)
        app.wait_exit()
        return left_at
    leave_and_check("force_quit", leave, slack=4)


def close_window():
    """The red button / ⌘W during playback, which quits the app."""
    def leave(app):
        app.wait_until_time(9.6)
        left_at = app.state()["time"]
        app.send("closeWindow")
        app.wait_exit()
        return left_at
    leave_and_check("close_window", leave)


def escape():
    """Échap / the back chevron, then quit."""
    def leave(app):
        app.wait_until_time(9.6)
        left_at = app.state()["time"]
        app.send("close")
        app.wait_for(lambda s: s.get("current") == "", "player to close")
        app.send("terminate")
        app.wait_exit()
        return left_at
    leave_and_check("escape", leave)


def resume_after_quit():
    """Quit mid-film, relaunch: playback picks up where it stopped."""
    support = os.path.join(WORK, "resume_after_quit")
    app = App(support, [FILM])
    try:
        app.wait_until_time(10)
        app.send("terminate")
        app.wait_exit()
    finally:
        app.kill()
    saved = app.records()[FILM]["position"]

    app = App(support, [FILM])
    try:
        state = app.wait_for(lambda s: s.get("time", 0) > 0 and s.get("isPlaying"),
                             "playback to start")
        check(state["time"] >= saved - 2,
              f"started at {state['time']:.1f}s instead of resuming near {saved:.1f}s")
        app.send("terminate")
        app.wait_exit()
    finally:
        app.kill()


def next_episode():
    """The 'épisode suivant' button counts the episode as seen."""
    support = os.path.join(WORK, "next_episode")
    app = App(support, [EP1, EP2])
    try:
        app.wait_until_time(5, EP1)
        app.send("next")
        app.wait_until_time(6, EP2)
        app.send("terminate")
        app.wait_exit()
    finally:
        app.kill()
    records = app.records()
    first = records.get(EP1, {})
    check(first.get("watched") is True, f"episode 1 not marked seen: {first}")
    check(not resumable(first), f"episode 1 still offered for resume: {first}")
    check(resumable(records.get(EP2)), f"episode 2 position lost: {records.get(EP2)}")


def end_of_file():
    """Playing to the end marks the episode seen and rolls on to the next."""
    support = os.path.join(WORK, "end_of_file")
    app = App(support, [EP1, EP2])
    try:
        app.wait_until_time(3, EP1)
        app.wait_for(lambda s: s.get("duration", 0) > 0, "duration")
        app.send(f"seek {SAMPLE_SECONDS - 4}")
        app.wait_until_time(2, EP2)
        app.send("terminate")
        app.wait_exit()
    finally:
        app.kill()
    check(app.records().get(EP1, {}).get("watched") is True,
          f"episode 1 not seen after playing to the end: {app.records().get(EP1)}")


def rewatch_resume():
    """A film seen before, restarted and left half way, resumes there."""
    support = os.path.join(WORK, "rewatch_resume")
    app = App(support, [FILM])
    try:
        app.wait_until_time(3)
        app.wait_for(lambda s: s.get("duration", 0) > 0, "duration")
        app.send(f"seek {SAMPLE_SECONDS - 3}")
        app.wait_for(lambda s: s.get("current") == "", "film to end")
        app.send("terminate")
        app.wait_exit()
    finally:
        app.kill()
    check(app.records().get(FILM, {}).get("watched") is True, "film not seen at the end")

    # Seen: it starts over from the top. Leave it at ~8s.
    app = App(support, [FILM])
    try:
        app.wait_until_time(8)
        app.send("terminate")
        app.wait_exit()
    finally:
        app.kill()
    record = app.records().get(FILM)
    check(record.get("watched") is True, f"rewatch: 'seen' lost: {record}")

    # Back again: it should pick up at ~8s, not start over once more.
    app = App(support, [FILM])
    try:
        state = app.wait_for(lambda s: s.get("time", 0) > 0 and s.get("isPlaying"),
                             "playback to start")
        check(state["time"] >= record["position"] - 2,
              f"rewatch restarted at {state['time']:.1f}s instead of "
              f"{record['position']:.1f}s")
        app.send("terminate")
        app.wait_exit()
    finally:
        app.kill()


SCENARIOS = [quit_menu, quit_keyboard, quit_fullscreen, quit_paused, quit_appleevent, sigterm, force_quit,
             close_window, escape, resume_after_quit, next_episode, end_of_file,
             rewatch_resume]


def main():
    wanted = set(sys.argv[1:])
    check(os.path.exists(BINARY), f"{BINARY} missing -- run Tests/E2E/run.sh")
    make_samples()
    failures = 0
    for scenario in SCENARIOS:
        if wanted and scenario.__name__ not in wanted:
            continue
        start = time.time()
        try:
            scenario()
            print(f"✓ {scenario.__name__}  ({time.time() - start:.0f}s)")
        except Exception as error:  # noqa: BLE001 -- report and carry on
            failures += 1
            print(f"✗ {scenario.__name__}: {error}")
    if failures == 0:
        shutil.rmtree(WORK, ignore_errors=True)
    else:
        print(f"\n{failures} échec(s) ; journaux et données dans {WORK}")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
