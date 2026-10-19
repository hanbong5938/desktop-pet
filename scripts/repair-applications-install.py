#!/usr/bin/env python3
"""One-time, explicit repair of the mistaken ~/Applications standalone install.

Requires Python 3.9+, macOS arm64, and the original signed app and sidecar.
Default: read-only preflight/dry-run (release downloads use a temporary directory).
Use --apply only after saving Pet drafts; --start separately requests manual Start.
Never reloads/stops Tern, adopts a profile, or removes original data.
"""

import argparse
import ctypes
import errno
import fcntl
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import plistlib
import pwd
import shutil
import socket
import stat
import struct
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.parse
import urllib.request
import uuid

VERSION = "0.4.13-beta.8"
APP = "DesktopPet.app"
BINARY = "Contents/MacOS/desktop-pet"
HELPER = "Contents/MacOS/desktop-pet-update-coordinator"
BASE = "https://github.com/hanbong5938/desktop-pet/releases/download/v" + VERSION
ARCHIVE = "DesktopPet-v" + VERSION + "-macos-arm64.tar.gz"
SIDECAR = "DesktopPet-v" + VERSION + "-rig-native.json"
MAX_JSON = 1024 * 1024
MAX_TREE = 2 * 1024 * 1024 * 1024


class Refused(Exception):
    pass


def require(condition, message):
    if not condition:
        raise Refused(message)


def absolute(value):
    require(isinstance(value, str) and value.startswith("/") and "\0" not in value,
            "an absolute path is required: " + repr(value))
    path = Path(value)
    require(str(path) == value and ".." not in path.parts and "." not in path.parts,
            "path must be exact and normalized: " + value)
    return path


def secure_path(path, home, missing=False):
    """No links or writable foreign components; system ancestors may be root-owned."""
    path = absolute(str(path))
    home = absolute(str(home))
    current = Path("/")
    for part in path.parts[1:]:
        current /= part
        try:
            info = current.lstat()
        except FileNotFoundError:
            require(missing, "missing required path: " + str(current))
            break
        require(not stat.S_ISLNK(info.st_mode), "symlink refused: " + str(current))
        private_staging_parent = (current == Path("/private/tmp") and home.parent == current and
                                  home.name.startswith("desktop-pet-repair-") and info.st_uid == 0 and
                                  stat.S_ISDIR(info.st_mode) and info.st_mode & stat.S_ISVTX)
        require(info.st_mode & 0o022 == 0 or private_staging_parent,
                "group/world-writable path refused: " + str(current))
        inside = current == home or home in current.parents
        require(info.st_uid == os.geteuid() if inside or current == path else
                info.st_uid in (0, os.geteuid()), "foreign owner: " + str(current))
        if current != path:
            require(stat.S_ISDIR(info.st_mode), "non-directory parent: " + str(current))
        elif stat.S_ISREG(info.st_mode):
            require(info.st_nlink == 1, "hard-linked file refused: " + str(current))
    return path


def unchanged_metadata(left, right):
    return all(getattr(left, key) == getattr(right, key) for key in
               ("st_dev", "st_ino", "st_mode", "st_nlink", "st_uid", "st_gid",
                "st_size", "st_mtime_ns", "st_ctime_ns"))


def read_bytes(path, maximum=MAX_JSON):
    info = path.lstat()
    require(stat.S_ISREG(info.st_mode) and not path.is_symlink() and info.st_nlink == 1,
            "not a single regular file: " + str(path))
    require(info.st_size <= maximum, "file exceeds size bound: " + str(path))
    with path.open("rb") as handle:
        data = handle.read(maximum + 1)
    require(len(data) <= maximum, "file grew beyond bound: " + str(path))
    require(unchanged_metadata(path.lstat(), info), "file changed while reading: " + str(path))
    return data


def no_duplicates(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON key: " + key)
        result[key] = value
    return result


def parse(data):
    try:
        return json.loads(data, object_pairs_hook=no_duplicates)
    except (ValueError, UnicodeError) as error:
        raise Refused("invalid JSON: " + str(error)) from error


def sha(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def tree_snapshot(root, home):
    secure_path(root, home)
    require(root.is_dir(), "not a directory: " + str(root))
    result = {}
    total = 0
    for directory, dirs, files in os.walk(root, followlinks=False):
        for name in dirs + files:
            path = Path(directory) / name
            secure_path(path, home)
            info = path.lstat()
            require(stat.S_ISDIR(info.st_mode) or stat.S_ISREG(info.st_mode) or
                    (stat.S_ISSOCK(info.st_mode) and name == "control.sock"),
                    "unsupported file in backup/closure: " + str(path))
            if stat.S_ISREG(info.st_mode):
                total += info.st_size
                require(total <= MAX_TREE and len(result) < 32768, "tree exceeds backup bound")
                result[str(path.relative_to(root))] = sha(path)
    return result


def member_spans(text):
    """Return top-level JSON value spans without rewriting unrelated SDK bytes."""
    decoder = json.JSONDecoder()
    cursor = 0
    while text[cursor].isspace():
        cursor += 1
    require(text[cursor] == "{", "SDK KV must be a JSON object")
    cursor += 1
    result = {}
    while True:
        while text[cursor].isspace():
            cursor += 1
        if text[cursor] == "}":
            return result
        key, cursor = decoder.raw_decode(text, cursor)
        while text[cursor].isspace():
            cursor += 1
        require(text[cursor] == ":", "invalid SDK KV member")
        cursor += 1
        while text[cursor].isspace():
            cursor += 1
        start = cursor
        _, cursor = decoder.raw_decode(text, cursor)
        result[key] = (start, cursor)
        while text[cursor].isspace():
            cursor += 1
        if text[cursor] == "}":
            return result
        require(text[cursor] == ",", "invalid SDK KV separator")
        cursor += 1


def saved_connection(data, expected, explicit=None):
    kv = parse(data)
    require(isinstance(kv, dict), "SDK KV is not an object")
    if "connection" in kv:
        connection = kv["connection"]
        require(isinstance(connection, dict) and type(connection.get("version")) is int and
                connection["version"] == 1 and
                set(connection) == {"version", "executable", "config_dir", "state_dir"},
                "invalid existing canonical SDK connection; no legacy fallback")
    else:
        require(all(key in kv for key in ("executable", "config_dir", "state_dir")),
                "legacy SDK route must contain all three saved fields")
        connection = {key: kv[key] for key in ("executable", "config_dir", "state_dir")}
    for key in ("executable", "config_dir", "state_dir"):
        absolute(connection[key])
    require(connection["executable"] == str(expected), "saved route is not the exact authorized executable")
    pair = (Path(connection["config_dir"]), Path(connection["state_dir"]))
    require(pair[0] != pair[1], "config and state directories must be distinct")
    require(explicit is None or pair == explicit, "explicit profile differs from saved SDK pair")
    return pair


def replace_saved_executable(data, old, new, pair):
    saved_connection(data, old, pair)
    text = data.decode("utf-8")
    spans = member_spans(text)
    offset = 0
    if "connection" in spans:
        start, end = spans["connection"]
        spans = member_spans(text[start:end])
        offset = start
    start, end = spans["executable"]
    changed = text[:offset + start] + json.dumps(str(new), ensure_ascii=False) + text[offset + end:]
    output = changed.encode("utf-8")
    saved_connection(output, new, pair)
    return output


def sync_directory(path):
    descriptor = os.open(path, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def durable_file(path, data):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "wb") as handle:
        handle.write(data)
        handle.flush()
        os.fsync(handle.fileno())
    sync_directory(path.parent)


def atomic_kv(path, before, after, home):
    secure_path(path, home)
    require(read_bytes(path) == before, "SDK KV changed concurrently; original and current bytes preserved")
    info = path.stat()
    temporary = path.with_name(".desktop-pet-repair-" + uuid.uuid4().hex)
    try:
        durable_file(temporary, after)
        os.chmod(temporary, stat.S_IMODE(info.st_mode))
        # This optimistic guard is deliberately not described as transactional CAS.
        require(unchanged_metadata(path.stat(), info) and read_bytes(path) == before,
                "SDK KV changed before replacement; refusing overwrite")
        os.replace(temporary, path)
        sync_directory(path.parent)
    finally:
        if temporary.exists():
            temporary.unlink()  # Only this exact script-created temporary file.


def authorize_locator(data, executable):
    locator = parse(data)
    require(isinstance(locator, dict) and set(locator) == {"version", "kind", "executable"} and
            type(locator.get("version")) is int and locator["version"] == 1 and
            locator.get("kind") == "standalone" and locator.get("executable") == str(executable),
            "locator is not the exact old standalone owner (Brew/source/custom refused)")


def require_absent(path):
    require(not os.path.lexists(path), "target conflict; preserve existing path: " + str(path))


def command(args, timeout=90, environment=None):
    try:
        result = subprocess.run([str(arg) for arg in args], stdin=subprocess.DEVNULL,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                timeout=timeout, env=environment, check=False)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise Refused("command did not complete: " + str(args[0]) + ": " + str(error)) from error
    require(len(result.stdout) <= 4 * MAX_JSON and len(result.stderr) <= 4 * MAX_JSON,
            "command output exceeds bound")
    require(result.returncode == 0, "command failed: " + str(args[0]) + " " +
            " ".join(str(arg) for arg in args[1:]) + "\n" + result.stderr.decode("utf-8", "replace")[-4096:])
    return result.stdout


def check_record(record, bundle, version=None):
    executable = bundle / BINARY
    require(isinstance(record, dict) and type(record.get("version")) is int and record["version"] == 1 and
            record.get("kind") == "standalone" and record.get("executable") == str(executable) and
            record.get("origin") == {"kind": "standalone", "bundle": str(bundle)} and
            record.get("image") == {"path": str(executable), "sha256": sha(executable)},
            "paired helper did not prove exact standalone owner/image")
    require(version is None or record.get("app_version") == version, "installed version mismatch")


def discover(bundle, version=None):
    record = parse(command([bundle / HELPER, "discover", "--executable", bundle / BINARY, "--json"], 240))
    check_record(record, bundle, version)
    return record


def validate_bundle(bundle, home, version=None, scratch=None):
    files = tree_snapshot(bundle, home)
    sidecar = Path(str(bundle) + ".rig-native.json")
    secure_path(sidecar, home)
    data = read_bytes(sidecar)
    record = parse(data)
    require(record.get("version") == 1 and record.get("paths_relative_to") == "Contents",
            "unsupported deployed sidecar")
    closure = record.get("deployed_sha256")
    require(isinstance(closure, dict) and closure, "missing full deployed closure")
    actual = {name[len("Contents/"):]: value for name, value in files.items() if name.startswith("Contents/")}
    require(len(actual) == len(files) and actual == closure, "full deployed file closure/hash mismatch")
    identity = record.get("distribution_identity")
    require(isinstance(identity, dict) and identity.get("distribution") == "neutral" and
            identity.get("app_name") == APP and identity.get("executable") == "desktop-pet" and
            identity.get("helper") == "desktop-pet-update-coordinator" and
            identity.get("bundle_id") == "io.github.hanbong5938.desktop-pet" and
            (version is None or identity.get("app_version") == version), "wrong release distribution identity")
    plist = plistlib.loads(read_bytes(bundle / "Contents/Info.plist"))
    require(plist.get("CFBundleIdentifier") == identity["bundle_id"] and
            plist.get("CFBundleExecutable") == "desktop-pet" and
            plist.get("CFBundleVersion") == identity["app_version"] and
            plist.get("CFBundleShortVersionString") == identity["app_version"], "bundle plist identity mismatch")
    command(["/usr/bin/codesign", "--verify", "--deep", "--strict", bundle])
    for relative in (BINARY, HELPER, "Contents/MacOS/rig-decode-worker", "Contents/Frameworks/libherdr_rig.dylib"):
        path = bundle / relative
        require(command(["/usr/bin/lipo", "-archs", path]).strip() == b"arm64", "not arm64-only: " + str(path))
        command(["/usr/bin/codesign", "--verify", "--strict", path])
    for relative in (BINARY, HELPER):
        binary = bundle / relative
        require(parse(command([binary, "distribution-identity"])) == identity, "paired build identity mismatch")
        capabilities = parse(command([binary, "update-capabilities"]))
        require(all(capabilities.get(key) == value for key, value in
                    (("protocol", 4), ("observation_mode", 2), ("integration_api", 1))), "wrong native capabilities")
    require(command([bundle / BINARY, "--version"]).strip() ==
            ("desktop-pet " + identity["app_version"]).encode(), "app version mismatch")
    pack = parse(read_bytes(bundle / "Contents/Resources/default/manifest.json"))
    require(pack.get("id") == "rubelia-at12-multi-pose" and pack.get("version") == 5 and
            pack.get("render_mode") == "rig", "unexpected default pack")
    if scratch is not None:
        validation = parse(command([bundle / BINARY, "pack", "validate", "--path",
                                    bundle / "Contents/Resources/default", "--config-dir", scratch / "pack-config",
                                    "--state-dir", scratch / "pack-state"], 240))
        require(validation.get("backend") == "rig", "native rig validation failed")
    require(tree_snapshot(bundle, home) == files and read_bytes(sidecar) == data,
            "bundle/sidecar changed during verification")
    return files, data


class ReleaseRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, message, headers, newurl):
        url = urllib.parse.urlsplit(newurl)
        require(url.scheme == "https" and url.hostname in
                {"github.com", "release-assets.githubusercontent.com", "objects.githubusercontent.com"} and
                url.port in (None, 443) and url.username is None, "non-official HTTPS release redirect refused")
        return super().redirect_request(request, fp, code, message, headers, newurl)


def download(name, target, maximum):
    opener = urllib.request.build_opener(ReleaseRedirects())
    deadline = time.monotonic() + 300
    with opener.open(BASE + "/" + name, timeout=15) as response, target.open("xb") as handle:
        size = 0
        while True:
            require(time.monotonic() < deadline, "release download exceeded deadline")
            chunk = response.read(1024 * 1024)
            if not chunk:
                break
            size += len(chunk)
            require(size <= maximum, "release asset exceeds size bound")
            handle.write(chunk)


def checksum(data, name):
    matches = []
    for line in data.decode("ascii").splitlines():
        fields = line.split()
        if len(fields) >= 2 and fields[1].lstrip("*") == name:
            require(len(fields) == 2 and len(fields[0]) == 64 and
                    all(ch in "0123456789abcdefABCDEF" for ch in fields[0]), "invalid release checksum")
            matches.append(fields[0].lower())
    require(len(matches) == 1, "missing/ambiguous release checksum: " + name)
    return matches[0]


def extract_archive(archive, destination):
    destination.mkdir(mode=0o700)
    with tarfile.open(archive, "r:gz") as source:
        entries = []
        for entry in source:
            entries.append(entry)
            require(len(entries) <= 32768, "archive entry bound exceeded")
        require(entries, "empty release archive")
        seen = set()
        total = 0
        for entry in entries:
            path = PurePosixPath(entry.name)
            require(entry.name == str(path) and not path.is_absolute() and ".." not in path.parts and
                    path.parts and path.parts[0] == APP and
                    (entry.isdir() or entry.isfile()) and not entry.issym() and not entry.islnk() and
                    entry.mode & 0o7022 == 0 and entry.size >= 0, "unsafe archive entry: " + entry.name)
            require(entry.name not in seen, "duplicate archive entry: " + entry.name)
            seen.add(entry.name)
            total += entry.size
            require(total <= MAX_TREE, "expanded archive exceeds size bound")
        # Never tarfile.extract: all names/types were validated and no links are admitted.
        for entry in sorted(entries, key=lambda item: len(PurePosixPath(item.name).parts)):
            target = destination / entry.name
            if entry.isdir():
                target.mkdir(mode=0o700, parents=True, exist_ok=True)
            else:
                target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
                with source.extractfile(entry) as incoming, target.open("xb") as outgoing:
                    shutil.copyfileobj(incoming, outgoing)
                os.chmod(target, entry.mode & 0o755)
    return destination / APP


def stage_release(scratch, home):
    sums = scratch / "SHA256SUMS"
    download("SHA256SUMS", sums, 65536)
    for name, maximum in ((ARCHIVE, 1024 * 1024 * 1024), (SIDECAR, MAX_JSON)):
        target = scratch / name
        download(name, target, maximum)
        require(sha(target) == checksum(read_bytes(sums, 65536), name), "release SHA-256 mismatch: " + name)
    candidate = extract_archive(scratch / ARCHIVE, scratch / "release")
    shutil.copyfile(scratch / SIDECAR, Path(str(candidate) + ".rig-native.json"))
    validate_bundle(candidate, home, VERSION, scratch)
    return candidate


class MacProcesses:
    """Bounded kernel executable/argv observation, not a native update reservation."""
    def __init__(self):
        self.lib = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
        self.lib.proc_pidpath.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_uint]
        self.libc = ctypes.CDLL(None, use_errno=True)

    def path(self, pid):
        buffer = ctypes.create_string_buffer(4096)
        result = self.lib.proc_pidpath(pid, buffer, len(buffer))
        if result <= 0:
            if ctypes.get_errno() == errno.ESRCH:
                return None
            raise Refused("cannot inspect executable of relevant PID " + str(pid))
        return os.fsdecode(buffer.value)

    def args(self, pid):
        mib = (ctypes.c_int * 3)(1, 49, pid)  # CTL_KERN, KERN_PROCARGS2
        buffer = ctypes.create_string_buffer(MAX_JSON)
        size = ctypes.c_size_t(len(buffer))
        require(self.libc.sysctl(mib, 3, buffer, ctypes.byref(size), None, 0) == 0,
                "cannot read complete argv/environment for PID " + str(pid))
        data = buffer.raw[:size.value]
        require(4 < len(data) < MAX_JSON, "truncated process arguments")
        argc = struct.unpack_from("i", data)[0]
        require(0 < argc <= 256, "invalid process argc")
        cursor = data.index(b"\0", 4) + 1
        while cursor < len(data) and data[cursor] == 0:
            cursor += 1
        args = []
        for _ in range(argc):
            end = data.index(b"\0", cursor)
            args.append(os.fsdecode(data[cursor:end]))
            cursor = end + 1
        return args

    def old_users(self, executable):
        # ps only enumerates names/UIDs; actual executable and argv come from the kernel.
        rows = command(["/bin/ps", "-axo", "pid=,uid=,comm="], 10).decode().splitlines()
        result = []
        for row in rows:
            fields = row.strip().split(None, 2)
            require(len(fields) == 3, "incomplete process listing")
            pid, uid, name = fields
            if Path(name).name not in ("desktop-pet", "herdr-desktop-pet"):
                continue
            pid = int(pid)
            path = self.path(pid)
            if path != str(executable):
                continue
            require(int(uid) == os.geteuid(), "old app is in use by another UID")
            args = self.args(pid)
            require(self.path(pid) == path, "process executable changed during inspection")
            result.append((pid, args))
        return result


def profile_args(pair):
    return ["--config-dir", pair[0], "--state-dir", pair[1]]


def authenticate_status(reply, executable, pair, processes, digest):
    require(isinstance(reply, dict) and reply.get("ok") is True and
            reply.get("command") == "status" and reply.get("executable_path") == str(executable) and
            reply.get("config_dir") == str(pair[0]) and reply.get("state_dir") == str(pair[1]),
            "status does not match the exact saved executable/profile")
    if reply.get("running"):
        require(type(reply.get("pid")) is int and reply["pid"] > 0 and reply.get("instance_id") and
                reply.get("running_sha256") == digest and processes.path(reply["pid"]) == str(executable),
                "runtime image/PID identity mismatch")
    return reply


def socket_json(path, request, pid=None):
    info = path.lstat()
    require(stat.S_ISSOCK(info.st_mode) and info.st_uid == os.geteuid() and info.st_mode & 0o077 == 0,
            "unsafe native control socket")
    deadline = time.monotonic() + 5
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(5)
        connection.connect(str(path))
        libc = ctypes.CDLL(None, use_errno=True)
        uid, gid = ctypes.c_uint(), ctypes.c_uint()
        require(libc.getpeereid(connection.fileno(), ctypes.byref(uid), ctypes.byref(gid)) == 0 and
                uid.value == os.geteuid(), "native control peer UID mismatch")
        peer = struct.unpack("i", connection.getsockopt(0, 2, 4))[0]  # SOL_LOCAL, LOCAL_PEERPID
        require(pid is None or peer == pid, "native control peer PID mismatch")
        connection.sendall(json.dumps(request).encode() + b"\n")
        data = b""
        while b"\n" not in data:
            remaining = deadline - time.monotonic()
            require(remaining > 0, "native reply deadline exceeded")
            connection.settimeout(remaining)
            chunk = connection.recv(65536)
            require(chunk, "incomplete native reply")
            data += chunk
            require(len(data) <= MAX_JSON, "oversized native reply")
        frame, extra = data.split(b"\n", 1)
        require(not extra, "unexpected extra native reply")
        return parse(frame), peer


def source_status(bundle, pair, processes):
    reply = parse(command([bundle / BINARY, "status"] + profile_args(pair)))
    authenticate_status(reply, bundle / BINARY, pair, processes, sha(bundle / BINARY))
    if reply.get("running"):
        direct, peer = socket_json(pair[1] / "control.sock", {"version": 1, "command": "status"}, reply["pid"])
        authenticate_status(direct, bundle / BINARY, pair, processes, sha(bundle / BINARY))
        require(direct.get("instance_id") == reply["instance_id"] and direct.get("pid") == peer,
                "source instance changed during authentication")
        reply = direct
    return reply


def check_old_users(users, status, pair, wrong_default=None):
    for pid, args in users:
        if (status.get("running") and pid == status["pid"]) or (wrong_default and pid == wrong_default["pid"]):
            continue
        # Only exact known short-lived queries to this route may drain naturally.
        tail = args[1:]
        safe = tail in (["update-capabilities"], ["distribution-identity"], ["--version"])
        safe = safe or tail in (["profile", "show", "--json"] + [str(x) for x in profile_args(pair)],
                                ["tern-exchange"] + [str(x) for x in profile_args(pair)],
                                ["status"] + [str(x) for x in profile_args(pair)])
        require(safe, "other active profile/mutator or unrecognized old-app process; no Stop sent: PID " + str(pid))


def neutral_default_pair(args, home):
    require(len(args) >= 2 and args[1] == "daemon" and len(args[2:]) % 2 == 0,
            "unrecognized other old-app process")
    values = {}
    for key, value in zip(args[2::2], args[3::2]):
        require(key in ("--host", "--config-dir", "--state-dir", "--assets") and key not in values,
                "unrecognized/duplicate other daemon argument")
        values[key] = value
    pair = (home / ".config/desktop-pet", home / ".local/state/desktop-pet")
    require(values.get("--config-dir") == str(pair[0]) and values.get("--state-dir") == str(pair[1]),
            "other daemon is not the exact mistaken neutral default profile: " + repr(values))
    return pair


def find_wrong_default(users, status, pair, old, home, processes):
    found = None
    for pid, args in users:
        if status.get("running") and pid == status["pid"]:
            continue
        if len(args) < 2 or args[1] != "daemon":
            continue
        default_pair = neutral_default_pair(args, home)
        require(default_pair != pair, "saved-profile daemon does not match authenticated status")
        require(found is None, "multiple other old-app daemons; no Stop sent")
        for path in default_pair:
            secure_path(path, home)
        found = source_status(old, default_pair, processes)
        require(found.get("running") is True and found.get("pid") == pid,
                "neutral default daemon PID/profile could not be authenticated")
    return found


def checked_stop(old, pair, status, before, home):
    output = command([old / BINARY, "stop"] + profile_args(pair), 60)
    # These releases' Stop CLI normally has no stdout: it checks the direct reply
    # internally and waits for lifecycle finalization before returning success.
    if output.strip():
        reply = parse(output)
        require(reply.get("command") == "stop" and reply.get("ok") is True and
                reply.get("shutdown") is True and reply.get("config_dir") == str(pair[0]) and
                reply.get("state_dir") == str(pair[1]), "unexpected Stop response")
    expected = before + (2 if status.get("running") else 1)
    require(generation(pair[1], home) == expected, "unexpected Stop generation; no install/restart attempted")
    return expected


def generation(state, home):
    result = 0
    for directory in ("updates", "updates-v4"):
        path = state / directory / "stop-generation.json"
        secure_path(path, home, missing=True)
        if path.exists():
            value = parse(read_bytes(path, 32))
            require(type(value) is int and 0 <= value < 2 ** 64, "invalid persisted Stop generation")
            result += value
    require(result < 2 ** 64, "Stop generation overflow")
    return result


def reject_pending_authority(home, states):
    guidance = "; reconcile with that operation's matching signed helper before repair; marker preserved: "
    for state in states:
        for namespace in ("updates", "updates-v4"):
            marker = state / namespace / "profile.reservation.json"
            secure_path(marker, home, missing=True)
            require(not os.path.lexists(marker), "pending profile update authority" + guidance + str(marker))
    for namespace in (".desktop-pet-updates", ".herdr-updates"):
        directory = home / namespace
        secure_path(directory, home, missing=True)
        if not directory.exists():
            continue
        index = directory / "installation-scopes.json"
        secure_path(index, home, missing=True)
        if index.exists():
            try:
                empty = parse(read_bytes(index)) == []
            except (Refused, ValueError):
                empty = False
            require(empty, "pending/invalid installation update scopes" + guidance + str(index))
        for marker in directory.glob("*.reservation.json"):
            raise Refused("pending installation update authority" + guidance + str(marker))


def backup_profiles(backup, pair, home):
    paths = list(pair) + [home / ".config/desktop-pet", home / ".local/state/desktop-pet"]
    for variable, default, suffix in (("XDG_CONFIG_HOME", home / ".config", "desktop-pet"),
                                      ("XDG_STATE_HOME", home / ".local/state", "desktop-pet")):
        base = absolute(os.environ[variable]) if os.environ.get(variable) else default
        paths.append(base / suffix)
    records = []
    for path in dict.fromkeys(paths):
        secure_path(path, home, missing=True)
        if not path.exists():
            records.append({"path": str(path), "backup": None})
            continue
        before = tree_snapshot(path, home)
        target = backup / ("profile-" + str(len(records)))
        shutil.copytree(path, target, ignore=shutil.ignore_patterns("control.sock"))
        require(tree_snapshot(target, home) == before and tree_snapshot(path, home) == before,
                "profile changed while backing up; no Stop sent")
        records.append({"path": str(path), "backup": str(target)})
    return records


def write_manifest(backup, manifest, phase):
    manifest["phase"] = phase
    temporary = backup / (".manifest-" + uuid.uuid4().hex)
    durable_file(temporary, (json.dumps(manifest, indent=2) + "\n").encode())
    os.replace(temporary, backup / "manifest.json")
    sync_directory(backup)


def move_original(source, destination, home):
    secure_path(source, home)
    require_absent(destination)
    os.rename(source, destination)
    sync_directory(source.parent)
    sync_directory(destination.parent)


def completed_backup(root, old, new, pair, home):
    if not root.exists():
        return None
    secure_path(root, home)
    for child in sorted(root.iterdir(), reverse=True):
        secure_path(child, home)
        path = child / "manifest.json"
        if not path.exists():
            continue
        secure_path(path, home)
        record = parse(read_bytes(path))
        if (record.get("phase") == "completed" and record.get("version") == VERSION and
                record.get("old") == str(old) and record.get("new") == str(new) and
                record.get("profile") == [str(x) for x in pair]):
            return child
    return None


def start_and_observe(new, pair, source, expected_generation, home, processes, environment):
    require(generation(pair[1], home) == expected_generation,
            "additional user Stop observed; installed app left stopped")
    arguments = [new / BINARY, "start"] + profile_args(pair)
    if source.get("running"):
        mode = source.get("observation_mode")
        require(mode in ("auto", "tern", "herdr"), "source host selection is not reproducible")
        arguments += ["--host", mode]
        if source.get("assets_override") is not None:
            assets = absolute(source["assets_override"])
            require(not (assets == home / "Applications" / APP or home / "Applications" / APP in assets.parents),
                    "assets override points inside moved original bundle")
            secure_path(assets, home)
            arguments += ["--assets", assets]
    print("Manual Start uses inherited Tern environment and explicit KV root; it is not a race-proof native update transaction.")
    started = parse(command(arguments, 90, environment))
    require(started.get("ok") is True and started.get("ready") is True and
            started.get("running") is True and started.get("executable_path") == str(new / BINARY) and
            started.get("config_dir") == str(pair[0]) and started.get("state_dir") == str(pair[1]) and
            started.get("running_sha256") == sha(new / BINARY), "manual Start did not confirm exact ready image/profile")
    deadline = time.monotonic() + 45
    last = None
    while time.monotonic() < deadline:
        require(generation(pair[1], home) == expected_generation,
                "user Stop observed during manual Start; no restart will be retried")
        last = source_status(new, pair, processes)
        if last.get("running") and last.get("ui_ready") is True and last.get("control_ready") is True:
            if source.get("running"):
                require(last.get("observation_mode") == source.get("observation_mode") and
                        last.get("assets_override") == source.get("assets_override"), "new runtime context mismatch")
            try:
                binding, _ = socket_json(pair[1] / "control.sock", {"version": 1, "kind": "tern-migration-evidence"}, last["pid"])
                if (binding.get("version") == 1 and binding.get("kind") == "tern-migration-evidence" and
                        binding.get("fresh") is True and binding.get("instance_id") == last["instance_id"] and
                        isinstance(binding.get("window_id"), str) and binding["window_id"] and
                        isinstance(binding.get("local_host"), str) and binding["local_host"] and
                        all(type(binding.get(key)) is int and binding[key] > 0 for key in
                            ("binding_epoch", "sequence", "observation_generation", "native_generation"))):
                    print("Installed beta.8; native UI/control ready; a real fresh Tern window binding is confirmed.")
                    print("Other Tern windows and semantic work were not independently verified.")
                    return True
            except (Refused, OSError):
                pass
        time.sleep(1)
    print("Installed beta.8, but Tern reconnection was NOT confirmed; no reload/Configure or retry was performed.")
    return False


def run(options):
    home = Path(pwd.getpwuid(os.geteuid()).pw_dir)
    require(platform.system() == "Darwin" and platform.machine() == "arm64", "repair requires macOS arm64")
    require(os.geteuid() != 0, "do not run repair as root or with sudo")
    require(not options.start or options.apply, "--start requires --apply")
    secure_path(home, home)
    old = home / "Applications" / APP
    base = home / ".local/share/desktop-pet"
    new = base / "libexec" / APP
    locator = base / "installation.json"
    backups = base / "repair-backups"
    tern_root = absolute(os.environ["TERN_CONFIG_DIR"]) if os.environ.get("TERN_CONFIG_DIR", "").startswith("/") else home / "Library/Application Support/Tern"
    kv = absolute(options.tern_kv) if options.tern_kv else tern_root / "plugin-data/tern-desktop-pet/kv.json"
    if options.tern_kv:
        require(kv.name == "kv.json" and kv.parent.name == "tern-desktop-pet" and kv.parent.parent.name == "plugin-data",
                "--tern-kv must identify plugin-data/tern-desktop-pet/kv.json")
        tern_root = kv.parent.parent.parent
    require(bool(options.config_dir) == bool(options.state_dir), "explicit profile arguments must be supplied together")
    explicit = (absolute(options.config_dir), absolute(options.state_dir)) if options.config_dir else None
    for path in (kv, locator):
        secure_path(path, home)
    kv_before, locator_before = read_bytes(kv), read_bytes(locator, 16384)
    if new.exists() and not os.path.lexists(old):
        require_absent(Path(str(old) + ".rig-native.json"))
        authorize_locator(locator_before, new / BINARY)
        pair = saved_connection(kv_before, new / BINARY, explicit)
        completed = completed_backup(backups, old, new, pair, home)
        require(completed is not None, "private target already exists without this repair's completed evidence; no mutation")
        validate_bundle(new, home, VERSION)
        discover(new, VERSION)
        print("Already repaired; no Stop/install/Start repeated. Backups: " + str(completed))
        print("Installation verified; current Tern connection was not independently checked.")
        if options.start:
            import shlex
            print("--start was not replayed. For a separate explicit Start, run:")
            print(shlex.join([str(new / BINARY), "start"] + [str(value) for value in profile_args(pair)]))
            return 3
        return 0
    require_absent(new)
    require_absent(Path(str(new) + ".rig-native.json"))
    secure_path(new.parent, home, missing=True)
    authorize_locator(locator_before, old / BINARY)
    pair = saved_connection(kv_before, old / BINARY, explicit)
    for path in pair:
        secure_path(path, home)
        require(path.is_dir(), "saved profile must already exist: " + str(path))
    states = [pair[1], home / ".local/state/desktop-pet"]
    reject_pending_authority(home, states)
    old_files, old_sidecar = validate_bundle(old, home)
    original = discover(old)
    processes = MacProcesses()
    status = source_status(old, pair, processes)
    users = processes.old_users(old / BINARY)
    wrong_default = find_wrong_default(users, status, pair, old, home, processes)
    check_old_users(users, status, pair, wrong_default)
    if options.start and status.get("running"):
        require(status.get("observation_mode") in ("auto", "tern", "herdr"),
                "source host selection cannot be preserved; no Stop sent")
        if status.get("assets_override") is not None:
            assets = absolute(status["assets_override"])
            require(not (assets == old or old in assets.parents),
                    "source assets override is inside old Applications bundle; no Stop sent")
            secure_path(assets, home)
    before_generation = generation(pair[1], home)
    wrong_pair = (home / ".config/desktop-pet", home / ".local/state/desktop-pet")
    wrong_generation = generation(wrong_pair[1], home) if wrong_default else None
    print("Mode: " + ("APPLY" if options.apply else "DRY RUN (no installation/KV/process changes)"))
    print("Old: " + str(old) + " (" + original["app_version"] + ")\nNew: " + str(new) + " (" + VERSION + ")")
    print("Exact saved profile: config=" + str(pair[0]) + " state=" + str(pair[1]))
    print("Source: " + ("running PID " + str(status["pid"]) if status.get("running") else "stopped"))
    if wrong_default:
        print("Also stopping only authenticated mistaken neutral-default Pet PID " + str(wrong_default["pid"]) +
              "; its profile data is retained and it will NOT be restarted.")
    print("KV: " + str(kv) + "\nBackups: " + str(backups))
    print("Save unsaved Pet drafts first. Do not change plugin settings during apply. Tern/windows/terminals/agents are never stopped.")
    print("Without --start the repaired Pet remains stopped. A byte-guarded KV edit is optimistic, not transactional CAS.")
    # /private/tmp is OS-owned sticky storage; the newly-created directory itself is private.
    with tempfile.TemporaryDirectory(prefix="desktop-pet-repair-", dir="/private/tmp") as directory:
        scratch = Path(directory)
        candidate = stage_release(scratch, scratch)
        print("Verified official beta.8 archive, full deployed closure, paired identities, strict signatures and native pack.")
        if not options.apply:
            print("Dry run complete. Re-run with --apply to perform the repair; add --start only for explicit manual Start.")
            return 0
        secure_path(base, home)
        backups.mkdir(mode=0o700, exist_ok=True)
        secure_path(backups, home)
        lock_path = backups / "repair.lock"
        if lock_path.exists():
            secure_path(lock_path, home)
        lock_fd = os.open(lock_path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
        with os.fdopen(lock_fd, "r+b") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            backup = backups / (time.strftime("%Y%m%dT%H%M%S") + "-" + uuid.uuid4().hex[:8])
            backup.mkdir(mode=0o700)
            manifest = {"version": VERSION, "old": str(old), "new": str(new), "kv": str(kv),
                        "profile": [str(x) for x in pair], "source": status, "original": original,
                        "wrong_default": wrong_default, "start_requested": options.start}
            try:
                durable_file(backup / "kv.original.json", kv_before)
                durable_file(backup / "installation.original.json", locator_before)
                manifest["profiles"] = backup_profiles(backup, pair, home)
                write_manifest(backup, manifest, "backed_up")
                require(read_bytes(kv) == kv_before and read_bytes(locator, 16384) == locator_before,
                        "KV/locator changed before Stop; no Stop sent")
                require(tree_snapshot(old, home) == old_files and
                        read_bytes(Path(str(old) + ".rig-native.json")) == old_sidecar, "original bundle changed before Stop")
                fresh = source_status(old, pair, processes)
                require(fresh.get("running") == status.get("running") and fresh.get("pid") == status.get("pid") and
                        fresh.get("instance_id") == status.get("instance_id"), "source runtime changed; no Stop sent")
                current_users = processes.old_users(old / BINARY)
                fresh_wrong = find_wrong_default(current_users, fresh, pair, old, home, processes)
                require((fresh_wrong is None) == (wrong_default is None) and
                        (fresh_wrong is None or
                         (fresh_wrong["pid"], fresh_wrong["instance_id"]) ==
                         (wrong_default["pid"], wrong_default["instance_id"])),
                        "neutral-default source changed before Stop")
                check_old_users(current_users, fresh, pair, fresh_wrong)
                require(generation(pair[1], home) == before_generation, "user Stop changed during preflight")
                reject_pending_authority(home, states)
                write_manifest(backup, manifest, "stopping")
                if wrong_default:
                    require(generation(wrong_pair[1], home) == wrong_generation,
                            "wrong-default Stop generation changed; no Stop sent")
                    checked_stop(old, wrong_pair, wrong_default, wrong_generation, home)
                    require(not source_status(old, wrong_pair, processes).get("running"),
                            "mistaken neutral-default daemon did not stop")
                expected_generation = checked_stop(old, pair, status, before_generation, home)
                manifest["stop_generation"] = expected_generation
                deadline = time.monotonic() + 45
                while processes.old_users(old / BINARY):
                    require(time.monotonic() < deadline, "old-app processes did not quiesce; no kill/install attempted")
                    time.sleep(0.25)
                require(not source_status(old, pair, processes).get("running"), "source daemon still running")
                lifecycle = pair[0] / "lifecycle.json"
                if lifecycle.exists():
                    secure_path(lifecycle, home)
                    require("startup" not in parse(read_bytes(lifecycle)), "pending startup; installation refused")
                lock_file = pair[1] / "desktop-pet.lock"
                secure_path(lock_file, home)
                with lock_file.open("r+b") as lifecycle_lock:
                    fcntl.flock(lifecycle_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    require_absent(new)
                    require_absent(Path(str(new) + ".rig-native.json"))
                    require(read_bytes(kv) == kv_before and read_bytes(locator, 16384) == locator_before and
                            tree_snapshot(old, home) == old_files and
                            read_bytes(Path(str(old) + ".rig-native.json")) == old_sidecar,
                            "source ownership/bytes changed before move")
                    require(not processes.old_users(old / BINARY), "new old-app process appeared before move")
                    reject_pending_authority(home, states)
                    write_manifest(backup, manifest, "moving_original")
                    move_original(old, backup / APP, home)
                    move_original(Path(str(old) + ".rig-native.json"), backup / (APP + ".rig-native.json"), home)
                    move_original(locator, backup / "installation.moved.json", home)
                    write_manifest(backup, manifest, "original_preserved")
                    request = scratch / "install-request.json"
                    durable_file(request, (json.dumps({"version": 1, "kind": "standalone", "staged_app": str(candidate)}) + "\n").encode())
                    write_manifest(backup, manifest, "installing")
                    installed = parse(command([candidate / HELPER, "install", "--request", request], 300))
                    check_record(installed, new, VERSION)
                    authorize_locator(read_bytes(locator, 16384), new / BINARY)
                    validate_bundle(new, home, VERSION)
                    discover(new, VERSION)
                    write_manifest(backup, manifest, "installed")
                    kv_after = replace_saved_executable(kv_before, old / BINARY, new / BINARY, pair)
                    durable_file(backup / "kv.repaired.json", kv_after)
                    atomic_kv(kv, kv_before, kv_after, home)
                    write_manifest(backup, manifest, "completed")
                print("Installed beta.8 and changed only the exact saved executable; original data retained at " + str(backup))
                if options.start:
                    environment = os.environ.copy()
                    environment["HOME"] = str(home)
                    environment["TERN_CONFIG_DIR"] = str(tern_root)
                    connected = start_and_observe(new, pair, status, expected_generation, home, processes, environment)
                    manifest["restart"] = "tern_binding_confirmed" if connected else "connection_not_confirmed"
                    write_manifest(backup, manifest, "completed")
                    return 0 if connected else 3
                print("Installed, not started; Tern reconnection is not claimed. No profile/default pointer was adopted or deleted.")
                return 0
            except BaseException as error:
                manifest["error"] = str(error)
                try:
                    phase = manifest.get("phase", "unknown")
                    write_manifest(backup, manifest, phase if phase == "completed" else "failed_at_" + phase)
                except OSError:
                    pass
                print("Repair stopped without blind rollback/restart. Old: " + str(old) + "\nNew: " + str(new) +
                      "\nRetained originals/evidence: " + str(backup), file=sys.stderr)
                raise


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--apply", action="store_true", help="explicitly authorize Pet-only Stop, private install and exact saved route edit")
    mode.add_argument("--dry-run", action="store_true", help="read-only preflight and verified release download (the default)")
    parser.add_argument("--start", action="store_true", help="separately authorize manual Start after repair; requires --apply")
    parser.add_argument("--tern-kv", metavar="ABS", help="actual custom Tern plugin-data/tern-desktop-pet/kv.json")
    parser.add_argument("--config-dir", metavar="ABS", help="optional check: must equal saved config directory")
    parser.add_argument("--state-dir", metavar="ABS", help="optional check: must equal saved state directory")
    options = parser.parse_args()
    try:
        return run(options)
    except (Refused, OSError, ValueError, KeyError, tarfile.TarError) as error:
        print("REFUSED: " + str(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
