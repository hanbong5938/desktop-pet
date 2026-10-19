import { afterEach, describe, expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { chmod, mkdir, mkdtemp, realpath, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const temporary: string[] = [];
afterEach(async () => {
  for (const path of temporary.splice(0)) await rm(path, { recursive: true, force: true });
});
async function fixture(): Promise<string> {
  const path = await mkdtemp(join(tmpdir(), "desktop-pet-profile-paths-"));
  temporary.push(path);
  return realpath(path);
}
function resolvePath(path: string) {
  return spawnSync("/bin/bash", ["-c", 'source "$1"; physical_profile_path "$2"', "profile-path-test", join(import.meta.dir, "profile-paths.sh"), path], {
    encoding: "utf8", env: { ...process.env, PATH: "/no-external-runtime", CDPATH: "/not-the-profile" },
  });
}
function separate(config: string, state: string, defaultConfig: string, defaultState: string) {
  return spawnSync("/bin/bash", ["-c", 'source "$1"; require_separate_development_profile "$2" "$3" "$4" "$5"', "profile-path-test", join(import.meta.dir, "profile-paths.sh"), config, state, defaultConfig, defaultState], {
    encoding: "utf8", env: { ...process.env, PATH: "/no-external-runtime" },
  });
}

describe("physical development profile isolation", () => {
  test("resolves separators, dots and parents without creating missing suffixes", async () => {
    const root = await fixture();
    await mkdir(join(root, "existing"));
    for (const [input, expected] of [
      ["/../../", "/"],
      [`${root}//existing/./`, `${root}/existing`],
      [`${root}/missing/../existing/absent//leaf/`, `${root}/existing/absent/leaf`],
    ]) {
      const result = resolvePath(input!);
      expect(result.status).toBe(0);
      expect(result.stdout).toBe(`${expected}\n`);
    }
    expect(existsSync(join(root, "missing"))).toBe(false);
    expect(existsSync(join(root, "existing/absent"))).toBe(false);
  });

  test("follows a symlink before processing dotdot, including a missing leaf", async () => {
    const root = await fixture();
    await mkdir(join(root, "physical/nested"), { recursive: true });
    await symlink(join(root, "physical/nested"), join(root, "alias"));
    const result = resolvePath(`${root}/alias/../new-leaf`);
    expect(result.status).toBe(0);
    expect(result.stdout).toBe(`${root}/physical/new-leaf\n`);
    expect(existsSync(join(root, "physical/new-leaf"))).toBe(false);
  });

  test("rejects config or state aliases of defaults, even when the leaf is missing", async () => {
    const root = await fixture();
    await mkdir(join(root, "default"));
    await symlink(join(root, "default"), join(root, "alias"));
    const config = `${root}/default/config`, state = `${root}/default/state`;
    for (const alias of [config, `${config}/`, `${root}//default/./config`, `${root}/default/extra/../config`, `${root}/alias/config`]) {
      const result = separate(alias, `${root}/dev-state`, config, state);
      expect(result.status).not.toBe(0);
      expect(result.stderr).toContain("development must use separate config and state directories");
    }
    expect(separate(`${root}/dev-config`, `${root}/alias/state`, config, state).status).not.toBe(0);
    expect(separate(`${root}/dev-config`, `${root}/dev-state`, config, state).status).toBe(0);
    expect(existsSync(`${root}/dev-config`)).toBe(false);
    expect(existsSync(`${root}/dev-state`)).toBe(false);
  });

  test("treats spaces and shell metacharacters literally", async () => {
    const root = await fixture();
    const literal = `${root}/space ; $(touch unexpected) [*] ' quote`;
    const result = resolvePath(literal);
    expect(result.status).toBe(0);
    expect(result.stdout).toBe(`${literal}\n`);
    expect(existsSync(literal)).toBe(false);
    expect(separate(literal, `${root}/state`, `${root}/default-config`, `${root}/default-state`).status).toBe(0);
  });

  test("rejects empty, relative, CRLF, file, dangling and looping components", async () => {
    const root = await fixture();
    await writeFile(join(root, "file"), "not a directory");
    await symlink(join(root, "absent"), join(root, "dangling"));
    await symlink(join(root, "loop-b"), join(root, "loop-a"));
    await symlink(join(root, "loop-a"), join(root, "loop-b"));
    await mkdir(join(root, "newline\n"));
    await symlink(join(root, "newline\n"), join(root, "newline-alias"));
    for (const path of ["", "relative", `${root}/line\nfeed`, `${root}/carriage\rreturn`, `${root}/file`, `${root}/file/..`, `${root}/dangling`, `${root}/dangling/leaf`, `${root}/loop-a/leaf`, `${root}/newline-alias`]) {
      expect(resolvePath(path).status).not.toBe(0);
    }
    expect(separate(`${root}/config`, `${root}/state`, "", `${root}/default-state`).status).not.toBe(0);
  });

  test("rejects inaccessible existing directory prefixes", async () => {
    const root = await fixture();
    const locked = join(root, "locked");
    await mkdir(locked);
    await chmod(locked, 0);
    try {
      expect(resolvePath(`${locked}/missing`).status).not.toBe(0);
    } finally {
      await chmod(locked, 0o700);
    }
  });
});
