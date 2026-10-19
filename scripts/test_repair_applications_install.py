"""Deterministic scratch coverage; never runs native apps or the canonical repair."""

import importlib.util
import io
import json
import os
from pathlib import Path
import tarfile
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "applications_repair", Path(__file__).with_name("repair-applications-install.py"))
repair = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(repair)


class RepairFilesystemTests(unittest.TestCase):
    def setUp(self):
        # Explicit scratch roots test primitives, not HOME-overridden native installs.
        self.temporary = tempfile.TemporaryDirectory(prefix="repair-test-", dir=Path(__file__).resolve().parent)
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.old = self.root / "Applications/DesktopPet.app"
        self.new = self.root / ".local/share/desktop-pet/libexec/DesktopPet.app"
        self.pair = (self.root / "tern/config", self.root / "tern/state")

    def file(self, relative, data=b"original"):
        path = self.root / relative
        path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        path.write_bytes(data)
        path.chmod(0o600)
        return path

    def route(self, legacy=False):
        connection = {"version": 1, "executable": str(self.old / repair.BINARY),
                      "config_dir": str(self.pair[0]), "state_dir": str(self.pair[1])}
        if legacy:
            connection.pop("version")
            connection["unrelated"] = {"message": "unchanged", "count": 9}
            return json.dumps(connection).encode()
        return ("{\n  \"unrelated\": [1, 2.00, {\"executable\": \"untouched\"}],\n  \"connection\": " +
                json.dumps(connection) + ",\n  \"unicode\": \"고양이\"\n}\n").encode()

    def test_only_exact_standalone_locator_is_authority(self):
        executable = self.old / repair.BINARY
        good = {"version": 1, "kind": "standalone", "executable": str(executable)}
        repair.authorize_locator(json.dumps(good).encode(), executable)
        for override in ({"kind": "homebrew"}, {"kind": "source"},
                         {"executable": str(self.new / repair.BINARY)}, {"version": 2},
                         {"version": True}, {"source_root": str(self.root)}):
            with self.subTest(override=override), self.assertRaises(repair.Refused):
                repair.authorize_locator(json.dumps(dict(good, **override)).encode(), executable)

    def test_target_conflict_and_dangling_symlink_are_preserved(self):
        target = self.file("private/existing", b"do not replace")
        with self.assertRaises(repair.Refused):
            repair.require_absent(target)
        self.assertEqual(target.read_bytes(), b"do not replace")
        link = target.with_name("dangling")
        link.symlink_to(target.with_name("missing"))
        with self.assertRaises(repair.Refused):
            repair.require_absent(link)
        self.assertTrue(link.is_symlink())

    def test_symlink_and_writable_components_refused_without_chmod(self):
        path = self.file("owned/record")
        repair.secure_path(path, self.root)
        link = path.with_name("link")
        link.symlink_to(path)
        with self.assertRaises(repair.Refused):
            repair.secure_path(link, self.root)
        path.parent.chmod(0o770)
        with self.assertRaises(repair.Refused):
            repair.secure_path(path, self.root)
        self.assertEqual(path.parent.stat().st_mode & 0o777, 0o770)
        self.assertEqual(path.read_bytes(), b"original")

    def test_hard_link_cannot_be_used_as_owned_file(self):
        original = self.file("data/original")
        linked = original.with_name("linked")
        os.link(original, linked)
        with self.assertRaises(repair.Refused):
            repair.read_bytes(linked)
        with self.assertRaises(repair.Refused):
            repair.secure_path(linked, self.root)

    def test_canonical_kv_change_preserves_every_other_byte(self):
        before = self.route()
        after = repair.replace_saved_executable(before, self.old / repair.BINARY,
                                                 self.new / repair.BINARY, self.pair)
        expected = before.replace(json.dumps(str(self.old / repair.BINARY)).encode(),
                                  json.dumps(str(self.new / repair.BINARY)).encode(), 1)
        self.assertEqual(after, expected)
        self.assertEqual(repair.saved_connection(after, self.new / repair.BINARY), self.pair)
        self.assertEqual(before, self.route())

    def test_complete_legacy_route_is_not_canonicalized_or_cleaned(self):
        before = self.route(legacy=True)
        after = repair.replace_saved_executable(before, self.old / repair.BINARY,
                                                 self.new / repair.BINARY, self.pair)
        self.assertEqual(after, before.replace(str(self.old / repair.BINARY).encode(),
                                               str(self.new / repair.BINARY).encode()))
        self.assertNotIn("connection", repair.parse(after))
        self.assertEqual(repair.parse(after)["unrelated"], repair.parse(before)["unrelated"])

    def test_route_requires_both_saved_profile_paths_and_exact_executable(self):
        good = self.route()
        for executable, pair in ((self.new / repair.BINARY, self.pair),
                                 (self.old / repair.BINARY, (self.pair[0], self.root / "other"))):
            with self.subTest(executable=executable, pair=pair), self.assertRaises(repair.Refused):
                repair.saved_connection(good, executable, pair)
        incomplete = json.dumps({"executable": str(self.old / repair.BINARY),
                                 "config_dir": str(self.pair[0])}).encode()
        with self.assertRaises(repair.Refused):
            repair.saved_connection(incomplete, self.old / repair.BINARY)
        malformed = repair.parse(self.route(legacy=True))
        malformed["connection"] = None
        with self.assertRaises(repair.Refused):
            repair.saved_connection(json.dumps(malformed).encode(), self.old / repair.BINARY)

    def test_duplicate_keys_never_authorize_route(self):
        for data in (b'{"connection":null,"connection":{}}',
                     b'{"connection":{"version":1,"version":1}}'):
            with self.subTest(data=data), self.assertRaises(repair.Refused):
                repair.parse(data)

    def test_observed_concurrent_kv_edit_is_not_overwritten(self):
        original = self.route()
        path = self.file("tern/plugin-data/tern-desktop-pet/kv.json", original)
        repaired = repair.replace_saved_executable(original, self.old / repair.BINARY,
                                                    self.new / repair.BINARY, self.pair)
        concurrent = original + b" \n"
        path.write_bytes(concurrent)
        with self.assertRaises(repair.Refused):
            repair.atomic_kv(path, original, repaired, self.root)
        self.assertEqual(path.read_bytes(), concurrent)
        self.assertEqual(list(path.parent.iterdir()), [path])

    def test_atomic_route_write_keeps_original_backup_and_permissions(self):
        before = self.route()
        path = self.file("tern/kv.json", before)
        backup = self.root / "kv.original.json"
        repair.durable_file(backup, before)
        after = repair.replace_saved_executable(before, self.old / repair.BINARY,
                                                 self.new / repair.BINARY, self.pair)
        repair.atomic_kv(path, before, after, self.root)
        self.assertEqual(path.read_bytes(), after)
        self.assertEqual(backup.read_bytes(), before)
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        with self.assertRaises(repair.Refused):
            repair.atomic_kv(path, before, after, self.root)
        self.assertEqual(path.read_bytes(), after)

    def test_whole_bundle_move_and_failure_keep_every_original_byte(self):
        self.file("Applications/DesktopPet.app/Contents/MacOS/desktop-pet", b"old executable")
        self.file("Applications/DesktopPet.app/Contents/Resources/character/asset", b"original asset")
        sidecar = self.file("Applications/DesktopPet.app.rig-native.json", b"original external record")
        backup = self.root / "backup"
        backup.mkdir(mode=0o700)
        before = repair.tree_snapshot(self.old, self.root)
        repair.move_original(self.old, backup / repair.APP, self.root)
        repair.move_original(sidecar, backup / (repair.APP + ".rig-native.json"), self.root)
        manifest = {"version": repair.VERSION, "old": str(self.old), "new": str(self.new),
                    "profile": [str(x) for x in self.pair]}
        repair.write_manifest(backup, manifest, "failed_at_installing")
        self.assertFalse(self.old.exists())
        self.assertEqual(repair.tree_snapshot(backup / repair.APP, self.root), before)
        self.assertEqual((backup / (repair.APP + ".rig-native.json")).read_bytes(), b"original external record")
        self.assertEqual(repair.parse((backup / "manifest.json").read_bytes())["phase"], "failed_at_installing")
        with self.assertRaises(repair.Refused):
            repair.move_original(self.old, backup / repair.APP, self.root)
        self.assertEqual(repair.tree_snapshot(backup / repair.APP, self.root), before)

    def test_completed_evidence_is_exact_and_failed_work_is_not_replayed(self):
        backups = self.root / "backups"
        backups.mkdir(mode=0o700)
        operation = backups / "one"
        operation.mkdir(mode=0o700)
        record = {"version": repair.VERSION, "old": str(self.old), "new": str(self.new),
                  "profile": [str(x) for x in self.pair]}
        repair.write_manifest(operation, record, "failed_at_installing")
        self.assertIsNone(repair.completed_backup(backups, self.old, self.new, self.pair, self.root))
        repair.write_manifest(operation, record, "completed")
        self.assertEqual(repair.completed_backup(backups, self.old, self.new, self.pair, self.root), operation)
        self.assertIsNone(repair.completed_backup(backups, self.old, self.new,
                                                 (self.pair[0], self.root / "other"), self.root))

    def test_native_owner_record_must_match_real_file_digest_and_exact_origin(self):
        executable = self.file("Applications/DesktopPet.app/Contents/MacOS/desktop-pet", b"signed fixture bytes")
        record = {"version": 1, "kind": "standalone", "executable": str(executable),
                  "app_version": repair.VERSION,
                  "origin": {"kind": "standalone", "bundle": str(self.old)},
                  "image": {"path": str(executable), "sha256": repair.sha(executable)}}
        repair.check_record(record, self.old, repair.VERSION)
        for changed in (dict(record, kind="homebrew"),
                        dict(record, origin={"kind": "source", "root": str(self.root)}),
                        dict(record, image={"path": str(executable), "sha256": "0" * 64}),
                        dict(record, app_version="0.4.12")):
            with self.subTest(record=changed), self.assertRaises(repair.Refused):
                repair.check_record(changed, self.old, repair.VERSION)

    def test_pending_update_authority_is_preserved_and_blocks_repair(self):
        state = self.root / "state"
        state.mkdir(mode=0o700)
        for relative, data in (
                ("state/updates-v4/profile.reservation.json", b'{"operation_id":"pending"}'),
                ("state/updates/profile.reservation.json", b'{"operation_id":"old"}'),
                (".desktop-pet-updates/installation-scopes.json", b'[{"pending":true}]'),
                (".herdr-updates/installation-scopes.json", b'invalid'),
                (".desktop-pet-updates/exact.reservation.json", b'{"operation_id":"pending"}')):
            with self.subTest(relative=relative):
                marker = self.file(relative, data)
                with self.assertRaises(repair.Refused):
                    repair.reject_pending_authority(self.root, [state])
                self.assertEqual(marker.read_bytes(), data)
                marker.unlink()
        empty = self.file(".desktop-pet-updates/installation-scopes.json", b"[]")
        repair.reject_pending_authority(self.root, [state])
        self.assertEqual(empty.read_bytes(), b"[]")

    def archive(self, entries):
        path = self.root / "release.tar.gz"
        with tarfile.open(path, "w:gz") as archive:
            for name, kind in entries:
                info = tarfile.TarInfo(name)
                info.mode = 0o755 if kind == "dir" else 0o644
                if kind == "dir":
                    info.type = tarfile.DIRTYPE
                    archive.addfile(info)
                elif kind in ("symlink", "hardlink"):
                    info.type = tarfile.SYMTYPE if kind == "symlink" else tarfile.LNKTYPE
                    info.linkname = "../../outside"
                    archive.addfile(info)
                else:
                    info.size = 7
                    archive.addfile(info, io.BytesIO(b"payload"))
        return path

    def test_archive_traversal_links_and_duplicates_are_rejected_before_extraction(self):
        for entries in ([('../outside', 'file')], [('/outside', 'file')],
                        [('DesktopPet.app/link', 'symlink')], [('DesktopPet.app/link', 'hardlink')],
                        [('Other.app/file', 'file')],
                        [('DesktopPet.app/file', 'file'), ('DesktopPet.app/file', 'file')]):
            with self.subTest(entries=entries):
                archive = self.archive(entries)
                destination = self.root / "unpacked"
                with self.assertRaises(repair.Refused):
                    repair.extract_archive(archive, destination)
                self.assertEqual(list(destination.iterdir()), [])
                destination.rmdir()
                archive.unlink()

    def test_regular_bounded_archive_extracts_without_foreign_ownership(self):
        archive = self.archive([('DesktopPet.app', 'dir'), ('DesktopPet.app/Contents/file', 'file')])
        bundle = repair.extract_archive(archive, self.root / "unpacked")
        self.assertEqual((bundle / "Contents/file").read_bytes(), b"payload")
        self.assertEqual((bundle / "Contents/file").stat().st_uid, os.geteuid())

    def test_only_proven_neutral_default_pair_is_eligible_for_extra_stop(self):
        correct = [str(self.old / repair.BINARY), "daemon", "--host", "tern", "--config-dir",
                   str(self.root / ".config/desktop-pet"), "--state-dir", str(self.root / ".local/state/desktop-pet")]
        self.assertEqual(repair.neutral_default_pair(correct, self.root),
                         (self.root / ".config/desktop-pet", self.root / ".local/state/desktop-pet"))
        for changed in (correct[:-1] + [str(self.pair[1])], correct + ["--state-dir", str(self.pair[1])],
                        [correct[0], "daemon"], correct + ["--unknown", "value"]):
            with self.subTest(changed=changed), self.assertRaises(repair.Refused):
                repair.neutral_default_pair(changed, self.root)

    def test_other_daemon_mutator_and_other_profile_query_cannot_be_ignored(self):
        executable = str(self.old / repair.BINARY)
        for args in ([executable, "daemon"], [executable, "profile", "adopt"],
                     [executable, "tern-exchange", "--config-dir", str(self.pair[0]), "--state-dir", "/other"]):
            with self.subTest(args=args), self.assertRaises(repair.Refused):
                repair.check_old_users([(123, args)], {"running": False}, self.pair)
        repair.check_old_users([(123, [executable, "tern-exchange"] +
                                 [str(x) for x in repair.profile_args(self.pair)])], {"running": False}, self.pair)

    def test_stop_generations_include_old_namespace_and_reject_invalid_values(self):
        state = self.root / "state"
        state.mkdir(mode=0o700)
        self.assertEqual(repair.generation(state, self.root), 0)
        self.file("state/updates/stop-generation.json", b"3")
        current = self.file("state/updates-v4/stop-generation.json", b"7")
        self.assertEqual(repair.generation(state, self.root), 10)
        for data in (b"true", b"-1", b"18446744073709551615"):
            current.write_bytes(data)
            with self.subTest(data=data), self.assertRaises(repair.Refused):
                repair.generation(state, self.root)


if __name__ == "__main__":
    unittest.main()
