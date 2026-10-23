import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { chmod, cp, mkdir, mkdtemp, readFile, readdir, realpath, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { isAbsolute, join, relative } from "node:path";
import { validateProductMetadata } from "./release-docs";

const root = join(import.meta.dir, "..");
const distribution = process.env.DESKTOP_PET_DISTRIBUTION ?? "neutral";
if (distribution !== "neutral" && distribution !== "legacy-0.3.16") {
  fail("DESKTOP_PET_DISTRIBUTION must be neutral or legacy-0.3.16");
}
const legacy = distribution === "legacy-0.3.16";
const cargoTarget = process.env.DESKTOP_PET_CARGO_TARGET?.trim() ?? "";
if (cargoTarget && !/^[A-Za-z0-9][A-Za-z0-9_.-]*$/.test(cargoTarget)) {
  fail(`invalid DESKTOP_PET_CARGO_TARGET: ${cargoTarget}`);
}
const targetDirectory = join(root, "native", "target", distribution);
const releaseDirectory = cargoTarget
  ? join(targetDirectory, cargoTarget, "release")
  : join(targetDirectory, "release");
// Cargo targets stay neutral; only the compiled legacy distribution envelope
// gives these same-source executables their bounded historical bundle names.
const nativeBinary = join(releaseDirectory, "desktop-pet");
const helperBinary = join(releaseDirectory, "desktop-pet-update-coordinator");
const assetsSource = join(root, "assets", "rubelia-default");
const thumbnailSource = join(root, "assets", "rubelia-thumbnail.png");
const rigBuild = join(targetDirectory, "rig-native", "release");
const rigBuildRecord = join(rigBuild, "rig-native.json");
let packageJson: { version?: unknown };
try {
  packageJson = JSON.parse(await readFile(join(root, "package.json"), "utf8")) as { version?: unknown };
} catch {
  fail("package.json is missing or invalid JSON");
}
const packageVersion = typeof packageJson.version === "string"
  ? packageJson.version
  : fail("package version must be a semantic version");
if (packageVersion !== "0.4.17-beta.9") {
  fail("the authorized neutral and legacy-0.3.16 distributions must use the same approved 0.4.17-beta.9 core revision");
}
await validateProductMetadata(root, packageVersion);
if (process.platform !== "darwin" || process.arch !== "arm64") fail("native packaging requires macOS arm64");
await requireFile(nativeBinary, "release native binary");
await requireFile(helperBinary, "release update coordinator");
await requireArm64(nativeBinary, "release native binary");
await requireArm64(helperBinary, "release update coordinator");
type BuildIdentity = {
  distribution: string; app_name: string; executable: string; helper: string;
  display_name: string; bundle_id: string; app_version: string;
  protocol: number; observation_mode: number; integration_api: number;
  core_revision: string;
  herdr_migration_revision?: string | null;
};
const buildIdentity = JSON.parse(await runTool([helperBinary, "distribution-identity"], "reading compiled distribution identity")) as BuildIdentity;
const expectedIdentity: Omit<BuildIdentity, "core_revision" | "herdr_migration_revision"> = {
  distribution, app_name: legacy ? "HerdrDesktopPet.app" : "DesktopPet.app",
  executable: legacy ? "herdr-desktop-pet" : "desktop-pet",
  helper: legacy ? "herdr-update-coordinator" : "desktop-pet-update-coordinator",
  display_name: legacy ? "Herdr Desktop Pet" : "Desktop Pet",
  bundle_id: legacy ? "dev.herdr.desktop-pet" : "io.github.hanbong5938.desktop-pet",
  app_version: legacy ? "0.3.16" : packageVersion,
  protocol: legacy ? 3 : 4, observation_mode: legacy ? 1 : 2, integration_api: 1,
};
for (const key of Object.keys(expectedIdentity) as (keyof typeof expectedIdentity)[]) {
  if (buildIdentity?.[key] !== expectedIdentity[key]) fail(`compiled ${distribution} identity disagrees on ${key}; rebuild in its separate target directory`);
}
const appIdentity = JSON.parse(await runTool([nativeBinary, "distribution-identity"], "reading compiled app identity")) as BuildIdentity;
for (const key of Object.keys(expectedIdentity) as (keyof typeof expectedIdentity)[]) {
  if (appIdentity?.[key] !== buildIdentity[key]) fail(`app/helper compiled identities disagree on ${key}`);
}
if (!/^[a-f0-9]{40}$/.test(buildIdentity.core_revision) ||
    appIdentity.core_revision !== buildIdentity.core_revision ||
    (await runTool(["git", "-C", root, "rev-parse", "HEAD"], "reading packaging source revision")).trim() !== buildIdentity.core_revision) {
  fail("packaged app/helper must be built from this exact core revision");
}
if (legacy) {
  if (typeof buildIdentity.herdr_migration_revision !== "string" ||
      buildIdentity.herdr_migration_revision !== "6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8" ||
      appIdentity.herdr_migration_revision !== buildIdentity.herdr_migration_revision ||
      process.env.DESKTOP_PET_HERDR_MIGRATION_REV !== buildIdentity.herdr_migration_revision) {
    fail("bridge app/helper must carry the exact authenticated integration-only managed-retirement revision");
  }
} else if (buildIdentity.herdr_migration_revision != null || appIdentity.herdr_migration_revision != null) {
  fail("neutral distribution must not carry legacy managed-retirement authority");
}
const appName = buildIdentity.app_name;
const outputDirectory = process.env.DESKTOP_PET_PACKAGE_STAGE_DIR ?? join(root, "dist");
if (!isAbsolute(outputDirectory)) fail("DESKTOP_PET_PACKAGE_STAGE_DIR must be absolute");
const appRoot = join(outputDirectory, appName);
const deployedRecord = join(outputDirectory, `${appName}.rig-native.json`);
const appContents = join(appRoot, "Contents");
const appBinary = join(appContents, "MacOS", buildIdentity.executable);
const appResources = join(appContents, "Resources");
const appBuildRecord = join(appResources, "rig-native-build.json");
const appHelper = join(appContents, "MacOS", buildIdentity.helper);
const appAssets = join(appResources, "default");
const appFrameworks = join(appContents, "Frameworks");
const appRigLibrary = join(appFrameworks, "libherdr_rig.dylib");
const appRigWorker = join(appContents, "MacOS", "rig-decode-worker");
const workerEntitlements = join(root, "native", "rig", "RigDecodeWorker.entitlements.plist");
const appCreator = join(appResources, "creator");
const licenseSource = join(root, "LICENSE.txt");
const licenseDestination = join(appResources, "LICENSE.txt");
const infoPlist = join(appContents, "Info.plist");

function fail(message: string): never {
  console.error(`package:native: ${message}`);
  process.exit(1);
}

async function requireFile(path: string, label: string): Promise<void> {
  try {
    const details = await stat(path);
    if (!details.isFile()) fail(`${label} is not a regular file: ${path}`);
  } catch {
    fail(`missing ${label}: ${path}; run the prerequisite command first`);
  }
}

async function requireDirectory(path: string, label: string): Promise<void> {
  try {
    const details = await stat(path);
    if (!details.isDirectory()) fail(`${label} is not a directory: ${path}`);
  } catch {
    fail(`missing ${label}: ${path}`);
  }
}


async function sha256(path: string): Promise<string> {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest("hex");
}

async function stageTernConsumer(): Promise<void> {
  // Only the neutral target delivers the integration; the legacy bridge never
  // substitutes mutable local consumer sources for the signed target package.
  if (legacy) return;
  const pin = JSON.parse(await readFile(join(root, "migration", "tern-consumer.json"), "utf8")) as {
    version: number; repository: string; revision: string; plugin_version: string;
    files: Record<string, string>;
  };
  const names = ["LICENSE.txt", "plugin.toml", "window.luau"];
  if (!pin || pin.version !== 1 || pin.repository !== "hanbong5938/tern-desktop-pet" ||
      !/^[a-f0-9]{40}$/.test(pin.revision) || pin.plugin_version !== "0.2.2-beta.2" ||
      !pin.files || Object.keys(pin.files).sort().join("\n") !== names.join("\n")) {
    fail("invalid immutable Tern consumer packaging authority");
  }
  const repository = `https://github.com/${pin.repository}`;
  const cargo = Bun.TOML.parse(await readFile(join(root, "native", "app", "Cargo.toml"), "utf8")) as {
    dependencies?: Record<string, { git?: unknown; rev?: unknown }>;
  };
  const lock = Bun.TOML.parse(await readFile(join(root, "native", "Cargo.lock"), "utf8")) as {
    package?: { name?: unknown; source?: unknown }[];
  };
  const adapter = cargo.dependencies?.["desktop-pet-tern"];
  const lockedAdapters = lock.package?.filter((entry) => entry.name === "desktop-pet-tern") ?? [];
  if (adapter?.git !== repository || adapter.rev !== pin.revision ||
      lockedAdapters.length !== 1 ||
      lockedAdapters[0]?.source !== `git+${repository}?rev=${pin.revision}#${pin.revision}`) {
    fail("bundled Tern consumer and compiled adapter must select the same immutable revision");
  }
  const destination = join(appResources, "integrations", "tern-desktop-pet");
  await mkdir(destination, { recursive: true });
  for (const name of names) {
    const expected = pin.files[name];
    if (typeof expected !== "string" || !/^[a-f0-9]{64}$/.test(expected)) {
      fail(`invalid Tern consumer digest: ${name}`);
    }
    const response = await fetch(
      `https://raw.githubusercontent.com/${pin.repository}/${pin.revision}/${name}`,
      { redirect: "error", signal: AbortSignal.timeout(30_000) },
    );
    if (!response.ok) fail(`immutable Tern consumer download failed: ${name} (${response.status})`);
    const bytes = new Uint8Array(await response.arrayBuffer());
    // Match the shipped coordinator's package_identity bounds before signing.
    const maximumBytes = name === "plugin.toml" ? 65_536 : 512 * 1024;
    if (bytes.byteLength === 0 || bytes.byteLength > maximumBytes ||
        createHash("sha256").update(bytes).digest("hex") !== expected) {
      fail(`immutable Tern consumer bytes differ from reviewed source: ${name}`);
    }
    if (name === "plugin.toml") {
      const manifest = Bun.TOML.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes)) as {
        schema?: unknown; id?: unknown; version?: unknown; window?: unknown;
      };
      if (manifest.schema !== 1 || manifest.id !== "tern-desktop-pet" ||
          manifest.version !== pin.plugin_version || manifest.window !== "window.luau") {
        fail("pinned Tern manifest disagrees with the signed production package");
      }
    }
    await writeFile(join(destination, name), bytes, { mode: 0o644 });
  }
  await writeFile(join(destination, "revision"), `${pin.revision}\n`, { mode: 0o644 });
}

async function filesUnder(directory: string): Promise<string[]> {
  const files: string[] = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) files.push(...await filesUnder(path));
    else if (entry.isFile()) files.push(path);
    else throw new Error(`unexpected bundle resource: ${path}`);
  }
  return files.sort();
}

async function identities(paths: string[], base: string): Promise<Record<string, string>> {
  const entries = await Promise.all(paths.map(async (path) => [relative(base, path), await sha256(path)]));
  return Object.fromEntries(entries.sort(([a], [b]) => a!.localeCompare(b!)));
}

function escapeXml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&apos;");
}

async function runTool(command: string[], label: string, input?: string): Promise<string> {
  const child = Bun.spawn(command, { stdin: input === undefined ? undefined : "pipe", stdout: "pipe", stderr: "pipe" });
  if (input !== undefined) {
    child.stdin.write(input);
    child.stdin.end();
  }
  const [exit, stdout, stderr] = await Promise.all([
    child.exited, new Response(child.stdout).text(), new Response(child.stderr).text(),
  ]);
  if (exit !== 0) throw new Error(`${label} failed (exit ${exit}): ${stderr.trim()}`);
  return stdout;
}

async function requireCapabilities(binary: string, label: string): Promise<void> {
  const capabilities = JSON.parse(await runTool([binary, "update-capabilities"], `${label} update capabilities`)) as Record<string, unknown> | null;
  if (capabilities === null || typeof capabilities !== "object" ||
      capabilities.protocol !== buildIdentity.protocol ||
      capabilities.observation_mode !== buildIdentity.observation_mode ||
      capabilities.integration_api !== buildIdentity.integration_api ||
      (legacy ? Object.hasOwn(capabilities, "migration_handover_protocol") : capabilities.migration_handover_protocol !== 1)) {
    fail(`${label} does not support the compiled ${distribution} updater/integration contract`);
  }
}

async function requireArm64(binary: string, label: string): Promise<void> {
  const architectures = (await runTool(["lipo", "-archs", binary], `inspecting ${label} architecture`)).trim();
  if (architectures !== "arm64") fail(`${label} must be arm64-only (got ${architectures})`);
}

async function validateStandaloneHelper(binary: string): Promise<void> {
  await requireArm64(binary, "update coordinator");
  await requireCapabilities(binary, "update coordinator");
  const commands = await runTool(["otool", "-l", binary], "inspecting update coordinator loader paths");
  if (/\bcmd LC_RPATH\b/.test(commands)) fail("update coordinator must not contain loader search paths");
  const dependencies = await runTool(["otool", "-L", binary], "inspecting update coordinator dependencies");
  const libraries = dependencies.split("\n").slice(1).map((line) => line.trim().split(/\s+/)[0]).filter(Boolean);
  if (!libraries.length || libraries.some((library) =>
    !library!.startsWith("/usr/lib/") && !library!.startsWith("/System/Library/"))) {
    fail(`update coordinator must depend only on system libraries: ${libraries.join(", ")}`);
  }
}

async function stripDevelopmentRpaths(binary: string, all = false): Promise<void> {
  const commands = await runTool(["otool", "-l", binary], "reading Mach-O load commands");
  for (const match of commands.matchAll(/cmd LC_RPATH\s+cmdsize \d+\s+path (.+?) \(offset \d+\)/g)) {
    const path = match[1]!;
    if (all || (!path.startsWith("@") && path !== "/usr/lib/swift")) {
      await runTool(["install_name_tool", "-delete_rpath", path, binary], "removing development loader path");
    }
  }
}

async function validateNativePack(binary: string, path: string): Promise<void> {
  const temporary = await mkdtemp(join(await realpath(tmpdir()), "desktop-pet-package-validation-"));
  try {
    await runTool([
      binary, "pack", "validate", "--path", path,
      "--config-dir", join(temporary, "config"), "--state-dir", join(temporary, "state"),
    ], `full native validation of ${path}`);
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
}

async function readEntitlements(target: string): Promise<Record<string, unknown>> {
  const plist = await runTool(["codesign", "-d", "--xml", "--entitlements", "-", target], `reading entitlements of ${target}`);
  if (!plist.trim()) return {};
  const json = await runTool(["plutil", "-convert", "json", "-o", "-", "-"], `parsing entitlements of ${target}`, plist);
  const value: unknown = JSON.parse(json);
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`invalid entitlements of ${target}: expected a dictionary`);
  }
  return value as Record<string, unknown>;
}

async function runCodesign(identity: string): Promise<void> {
  for (const target of [appRigLibrary, appRigWorker, appHelper, appBinary, appRoot]) {
    const command = ["codesign", "--force", "--timestamp=none", "--sign", identity];
    if (target === appRigWorker) command.push("--entitlements", workerEntitlements);
    await runTool([...command, target], `signing ${target}`);
  }
  await runTool(["codesign", "--verify", "--strict", appHelper], "verifying signed update coordinator");
  await runTool(["codesign", "--verify", "--strict", appRigWorker], "verifying packaged decode worker signature");
  await runTool(["codesign", "--verify", "--deep", "--strict", appRoot], "verifying packaged signature");
  const worker = await readEntitlements(appRigWorker);
  const keys = Object.keys(worker);
  if (keys.length !== 1 || keys[0] !== "com.apple.security.cs.allow-jit" || worker[keys[0]] !== true) {
    throw new Error("packaged decode worker must have only com.apple.security.cs.allow-jit=true");
  }
  for (const target of [appRigLibrary, appHelper, appBinary, appRoot]) {
    if (Object.keys(await readEntitlements(target)).length !== 0) {
      throw new Error(`unexpected entitlements on ${target}`);
    }
  }
}

async function main(): Promise<void> {
  if (process.platform !== "darwin") fail("native packaging requires macOS");
  if (process.arch !== "arm64") fail("native packaging requires macOS arm64");
  if (!Bun.which("codesign")) fail("codesign is required but was not found on PATH");

  await requireFile(licenseSource, "license notice");
  await requireFile(nativeBinary, "release native binary");
  await requireFile(helperBinary, "release update coordinator");
  await requireArm64(nativeBinary, "release native binary");
  await requireCapabilities(nativeBinary, "release native binary");
  await requireCapabilities(helperBinary, "release update coordinator");
  await requireDirectory(assetsSource, "Rubelia rig default");
  await requireFile(thumbnailSource, "Rubelia menu thumbnail");
  await requireFile(join(rigBuild, "libherdr_rig.dylib"), "release native rig library");
  await requireFile(join(rigBuild, "rig-decode-worker"), "release native decode worker");
  await requireFile(workerEntitlements, "decode worker JIT entitlements");
  await requireFile(join(rigBuild, "Resources", "rig", "decoder.js"), "trusted decoder bundle");
  await requireFile(join(rigBuild, "Resources", "rig", "NOTICE.txt"), "renderer license closure");
  await requireFile(join(rigBuild, "Resources", "rig", "RigLimits.swift"), "generated Swift rig limits");
  await requireFile(join(rigBuild, "Resources", "rig", "RigLimits.rs"), "generated Rust rig limits");
  await requireFile(join(root, "native", "rig", "limits.json"), "rig limits source");
  await requireFile(rigBuildRecord, "native rig build record");
  await requireFile(join(root, "tools", "character-pack.py"), "creator tool");
  await requireFile(join(root, ".agents", "skills", "character-creator", "SKILL.md"), "creator skill");
  const buildRecord = JSON.parse(await readFile(rigBuildRecord, "utf8")) as {
    version?: number;
    build?: { distribution?: string; profile?: string; architecture?: string; source_sha256?: Record<string, string>; output_sha256?: Record<string, string> };
  };
  const expectedOutputs = buildRecord.build?.output_sha256;
  if (buildRecord.version !== 1 || buildRecord.build?.distribution !== distribution ||
      buildRecord.build.profile !== "release" || buildRecord.build.architecture !== "arm64" ||
      !expectedOutputs || !buildRecord.build.source_sha256) {
    fail("native rig build record lacks matching distribution/release/arm64 source/output identities; rebuild the native rig");
  }
  const sourcePaths = Object.keys(buildRecord.build.source_sha256).sort();
  if (!sourcePaths.length || sourcePaths.some((path) => path.startsWith("/") || path.split("/").includes(".."))) {
    fail("native rig build record has invalid source paths");
  }
  for (const path of sourcePaths) {
    if (buildRecord.build.source_sha256[path] !== await sha256(join(root, path))) {
      fail(`native rig build source differs from its build record: ${path}`);
    }
  }
  const rigBuildResourcePaths = (await filesUnder(join(rigBuild, "Resources", "rig")))
    .map((path) => relative(rigBuild, path));
  const buildPaths = ["libherdr_rig.dylib", "rig-decode-worker", ...rigBuildResourcePaths];
  if (Object.keys(expectedOutputs).sort().join("\n") !== buildPaths.sort().join("\n")) {
    fail("native rig build resource closure differs from its build record");
  }
  for (const path of buildPaths) {
    if (expectedOutputs[path] !== await sha256(join(rigBuild, path))) {
      fail(`native rig build output differs from its build record: ${path}`);
    }
  }
  await validateNativePack(nativeBinary, assetsSource);

  const displayName = escapeXml(buildIdentity.display_name);
  const bundleIdentifier = escapeXml(buildIdentity.bundle_id);
  const version = escapeXml(buildIdentity.app_version);
  const versionProcess = Bun.spawn([nativeBinary, "--version"], {
    stdout: "pipe",
    stderr: "pipe",
  });
  const [versionExit, versionOutput, versionError] = await Promise.all([
    versionProcess.exited,
    new Response(versionProcess.stdout).text(),
    new Response(versionProcess.stderr).text(),
  ]);
  if (versionExit !== 0 || versionOutput.trim() !== `${buildIdentity.executable} ${buildIdentity.app_version}`) {
    fail(`release binary version does not match compiled identity; rebuild before packaging: ${versionError.trim()}`);
  }

  await rm(appRoot, { recursive: true, force: true });
  await rm(deployedRecord, { force: true });
  await mkdir(join(appContents, "MacOS"), { recursive: true });
  await mkdir(appResources, { recursive: true });
  await mkdir(appFrameworks, { recursive: true });
  await mkdir(appCreator, { recursive: true });
  await cp(nativeBinary, appBinary);
  await chmod(appBinary, 0o755);
  await cp(helperBinary, appHelper);
  await chmod(appHelper, 0o755);
  await cp(licenseSource, licenseDestination);
  await cp(assetsSource, appAssets, { recursive: true });
  await cp(thumbnailSource, join(appResources, "default-thumbnail.png"));
  await cp(rigBuildRecord, appBuildRecord);
  await cp(join(rigBuild, "libherdr_rig.dylib"), appRigLibrary);
  await cp(join(rigBuild, "rig-decode-worker"), appRigWorker);
  await chmod(appRigWorker, 0o755);
  await mkdir(join(appResources, "rig"), { recursive: true });
  for (const resource of ["decoder.js", "RigLimits.swift", "RigLimits.rs", "NOTICE.txt", "vendor"]) {
    await cp(join(rigBuild, "Resources", "rig", resource), join(appResources, "rig", resource), { recursive: true });
  }
  await cp(join(root, "native", "rig", "limits.json"), join(appResources, "rig", "limits.json"));
  await cp(join(root, "tools", "character-pack.py"), join(appCreator, "character-pack.py"));
  await cp(join(root, ".agents", "skills", "character-creator", "SKILL.md"), join(appCreator, "SKILL.md"));
  await stageTernConsumer();
  for (const binary of [appBinary, appRigLibrary, appRigWorker]) {
    await stripDevelopmentRpaths(binary);
  }
  await stripDevelopmentRpaths(appHelper, true);
  await validateStandaloneHelper(appHelper);

  const plist = `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleLocalizations</key>
  <array>
    <string>ko</string>
    <string>en</string>
  </array>
  <key>CFBundleDisplayName</key>
  <string>${displayName}</string>
  <key>CFBundleExecutable</key>
  <string>${escapeXml(buildIdentity.executable)}</string>
  <key>CFBundleIdentifier</key>
  <string>${bundleIdentifier}</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>${displayName}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${version}</string>
  <key>CFBundleVersion</key>
  <string>${version}</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>LSUIElement</key>
  <true/>
</dict>
</plist>
`;
  await writeFile(infoPlist, plist, "utf8");

  const identity =
    process.env.DESKTOP_PET_CODESIGN_IDENTITY?.trim() ||
    process.env.CODESIGN_IDENTITY?.trim() ||
    "-";
  await runCodesign(identity);
  await requireCapabilities(appBinary, "packaged native binary");
  await validateStandaloneHelper(appHelper);
  await validateNativePack(appBinary, appAssets);
  // External post-signing record avoids a signed-bundle self-hash cycle.
  const packagedRecord = {
    version: 1,
    distribution_identity: buildIdentity,
    core_revision: buildIdentity.core_revision,
    paths_relative_to: "Contents",
    dylib: "Frameworks/libherdr_rig.dylib",
    worker: "MacOS/rig-decode-worker",
    coordinator: `MacOS/${buildIdentity.helper}`,
    generated_limits: ["Resources/rig/RigLimits.swift", "Resources/rig/RigLimits.rs"],
    decoder: "Resources/rig/decoder.js",
    limits: "Resources/rig/limits.json",
    install_name: "@rpath/libherdr_rig.dylib",
    packaged_rpath: "@executable_path/../Frameworks",
    probe_faults: false,
    notices: ["Resources/rig/NOTICE.txt", "Resources/rig/vendor"],
    build_record: "Resources/rig-native-build.json",
    deployed_sha256: await identities(await filesUnder(appContents), appContents),
  };
  await writeFile(deployedRecord, JSON.stringify(packagedRecord, null, 2) + "\n", "utf8");
  if (identity === "-") {
    console.log(`Packaged ${appRoot} with an ad-hoc local signature; no official signing or notarization was performed.`);
  } else {
    console.log(`Packaged ${appRoot} with signing identity ${identity}; notarization was not performed.`);
  }
}

await main();
