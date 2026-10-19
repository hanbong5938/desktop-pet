import { spawnSync } from "node:child_process"
import { createHash } from "node:crypto"
import { createReadStream } from "node:fs"
import { chmod, copyFile, lstat, mkdir, readdir, realpath, writeFile } from "node:fs/promises"
import { dirname, isAbsolute, join, relative, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { writeRigLimits } from "../native/rig/build-limits.mjs"

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..")
const accepted = new Set(["--source", "--runtime", "--assets", "--expect", "--output"])
const commands = []
let outputDirectory
let failureStatus = 1

async function hash(path) {
  const digest = createHash("sha256")
  for await (const chunk of createReadStream(path)) digest.update(chunk)
  return digest.digest("hex")
}
async function files(directory) {
  const result = []
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name)
    if (entry.isDirectory()) result.push(...await files(path))
    else if (entry.isFile()) result.push(path)
    else throw new Error(`runtime contains a non-regular entry: ${path}`)
  }
  return result.sort()
}
async function copyRegular(source, destination) {
  const info = await lstat(source)
  if (!info.isFile() || info.isSymbolicLink()) throw new Error(`not a regular runtime file: ${source}`)
  await mkdir(dirname(destination), { recursive: true })
  await copyFile(source, destination)
  await chmod(destination, info.mode & 0o777)
  const sha256 = await hash(source)
  if (await hash(destination) !== sha256) throw new Error(`runtime copy mismatch: ${source}`)
  return { source, destination, sha256, mode: info.mode & 0o777 }
}
async function run(label, program, args) {
  const child = spawnSync(program, args, { cwd: root, encoding: "utf8", maxBuffer: 32 * 1024 * 1024 })
  const record = { label, program, args, status: child.status, signal: child.signal,
    error: child.error?.message, stdout: child.stdout ?? "", stderr: child.stderr ?? "" }
  commands.push(record)
  await writeFile(join(outputDirectory, `${label}.stdout.txt`), record.stdout)
  await writeFile(join(outputDirectory, `${label}.stderr.txt`), record.stderr)
  await writeFile(join(outputDirectory, "commands.json"), JSON.stringify(commands, null, 2))
  if (child.error || child.status !== 0) {
    failureStatus = child.status && child.status > 0 ? child.status : 1
    throw new Error(`${label} failed (${child.status ?? child.signal ?? "spawn"}): ${record.error ?? record.stderr}`)
  }
  return record.stdout.trim()
}

try {
  const options = new Map()
  const args = process.argv.slice(2)
  for (let i = 0; i < args.length; i += 2) {
    const key = args[i], value = args[i + 1]
    if (!accepted.has(key) || options.has(key) || !value || value.startsWith("--")) {
      throw new Error(`missing, duplicate, or unknown option: ${key}`)
    }
    options.set(key, value)
  }
  if (options.size !== accepted.size) throw new Error("required: --source ABS --runtime ABS --assets ABS --expect existing-hidden-render|hidden-stopped --output ABS")
  const expectation = options.get("--expect")
  if (!["existing-hidden-render", "hidden-stopped"].includes(expectation)) throw new Error(`invalid expectation: ${expectation}`)
  for (const key of ["--source", "--runtime", "--assets", "--output"]) {
    if (!isAbsolute(options.get(key))) throw new Error(`${key} must be absolute`)
  }
  const source = await realpath(options.get("--source"))
  const runtime = await realpath(options.get("--runtime"))
  const assets = await realpath(options.get("--assets"))
  const output = resolve(options.get("--output"))
  const existing = await lstat(output).catch(error => { if (error.code === "ENOENT") return null; throw error })
  if (existing && (!existing.isDirectory() || existing.isSymbolicLink() || (await readdir(output)).length !== 0)) {
    throw new Error("--output must be a new or empty non-symlink directory")
  }
  await mkdir(output, { recursive: true })
  outputDirectory = output
  const copied = [await copyRegular(join(runtime, "rig-decode-worker"), join(output, "rig-decode-worker"))]
  const runtimeResources = join(runtime, "Resources", "rig")
  const resourceInfo = await lstat(runtimeResources)
  if (!resourceInfo.isDirectory() || resourceInfo.isSymbolicLink()) throw new Error("runtime Resources/rig must be a real directory")
  for (const path of await files(runtimeResources)) {
    copied.push(await copyRegular(path, join(output, "Resources", "rig", relative(runtimeResources, path))))
  }
  const limits = join(output, "Resources", "rig", "RigLimits.swift")
  await writeRigLimits(limits)
  const swiftc = await run("find-swiftc", "xcrun", ["--find", "swiftc"])
  const sdk = await run("find-sdk", "xcrun", ["--sdk", "macosx", "--show-sdk-path"])
  const compilerVersion = await run("swiftc-version", swiftc, ["--version"])
  const sdkVersion = await run("sdk-version", "xcrun", ["--sdk", "macosx", "--show-sdk-version"])
  await run("verify-worker-signature", "/usr/bin/codesign", ["--verify", "--strict", join(output, "rig-decode-worker")])
  const sources = ["RigModel.swift", "RigDecoder.swift", "RigDeformation.swift", "RigHitTesting.swift",
    "RigMetalShaders.swift", "RigMetalRenderer.swift", "RigMotion.swift", "RigNativeHost.swift"]
    .map(name => join(source, "native", "rig", name))
  const header = join(source, "native", "rig", "RigBridge.h")
  const harness = join(root, "tools", "rig-host-visibility-check", "main.swift")
  const generated = [limits, join(dirname(limits), "RigLimits.rs")]
  const identities = await Promise.all([...sources, header, harness, ...generated,
    join(root, "scripts", "check-rig-host-visibility.mjs"), join(root, "native", "rig", "build-limits.mjs"),
    join(root, "native", "rig", "limits.json")].map(async path => ({ path, sha256: await hash(path) })))
  await writeFile(join(output, "manifest.json"), JSON.stringify({ source, runtime, assets, expectation,
    swiftc, compilerSHA256: await hash(swiftc), compilerVersion, sdk, sdkVersion, sources: identities,
    runtimeCopies: copied, generated }, null, 2))
  const binary = join(output, "rig-host-visibility-check")
  const flags = ["-sdk", sdk, "-target", "arm64-apple-macos13.0", "-O", "-whole-module-optimization",
    "-import-objc-header", header]
  if (expectation === "hidden-stopped") flags.push("-D", "RIG_HOST_VISIBILITY_CHECKS")
  await run("compile", swiftc, [...flags, ...sources, limits, harness,
    ...["AppKit", "CoreGraphics", "Foundation", "ImageIO", "JavaScriptCore", "Metal", "QuartzCore"]
      .flatMap(name => ["-framework", name]), "-o", binary])
  const stdout = await run("visibility", binary, ["--assets", assets, "--expect", expectation,
    "--output", join(output, "results")])
  process.stdout.write(`${stdout}\n`)
} catch (error) {
  process.stderr.write(`rig-host-visibility-check: ${error.message}\n`)
  if (outputDirectory) await writeFile(join(outputDirectory, "failure.json"), JSON.stringify({ error: error.message, status: failureStatus }, null, 2))
  process.exitCode = failureStatus
}
