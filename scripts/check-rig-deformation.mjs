#!/usr/bin/env node
import { createHash } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import { mkdir, readdir, readFile, writeFile, stat } from 'node:fs/promises'
import { dirname, isAbsolute, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { writeRigLimits } from '../native/rig/build-limits.mjs'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const sourceNames = ['RigModel.swift', 'RigMotion.swift', 'RigDeformation.swift', 'RigHitTesting.swift']
const manifest = { schema: 1, commands: [], files: [], performanceClaim: false }
let output
let sequence = 0
async function saveManifest() {
  if (output) await writeFile(join(output, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n')
}
async function hash(path, role) {
  const bytes = await readFile(path)
  const entry = { role, path, bytes: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') }
  manifest.files.push(entry)
  return entry
}
async function run(label, executable, args) {
  const id = `${String(++sequence).padStart(3, '0')}-${label}`
  const startedAt = new Date().toISOString()
  const result = spawnSync(executable, args, { cwd: root, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 })
  const stdout = result.stdout ?? '', stderr = result.stderr ?? ''
  await writeFile(join(output, `${id}.stdout.txt`), stdout)
  await writeFile(join(output, `${id}.stderr.txt`), stderr)
  const record = { label, executable, args, startedAt, finishedAt: new Date().toISOString(), status: result.status, signal: result.signal,
    error: result.error?.message, stdout: `${id}.stdout.txt`, stderr: `${id}.stderr.txt` }
  manifest.commands.push(record)
  await saveManifest()
  if (result.error || result.signal || result.status !== 0) {
    const error = new Error(`${label} failed (${result.error?.message ?? result.signal ?? result.status})\n${stderr}`)
    error.exitStatus = Number.isInteger(result.status) && result.status > 0 ? result.status : 1
    throw error
  }
  return stdout
}
function parseOptions() {
  const options = new Map()
  const args = process.argv.slice(2)
  for (let i = 0; i < args.length; i += 2) {
    const key = args[i], value = args[i + 1]
    if (!['--baseline-source', '--output'].includes(key) || options.has(key) || !value || value.startsWith('--') || !isAbsolute(value)) {
      throw new Error('usage: node scripts/check-rig-deformation.mjs [--baseline-source ABS] --output ABS (unique options; absolute paths)')
    }
    options.set(key, resolve(value))
  }
  if (!options.has('--output')) throw new Error('--output ABS is required')
  return options
}
function parseBenchmark(text, label) {
  const result = JSON.parse(text)
  if (!Array.isArray(result) || result.length !== 4) throw new Error(`${label}: expected four benchmark workloads`)
  const names = ['mixed', 'front', 'back', 'attached']
  for (let i = 0; i < names.length; i++) {
    const item = result[i]
    if (item.workload !== names[i] || typeof item.checksum !== 'string' || !/^[0-9a-f]+$/.test(item.checksum) ||
      !Number.isFinite(item.nsPerUpdate) || item.nsPerUpdate <= 0 || !Number.isFinite(item.batchNanoseconds) || item.batchNanoseconds <= 0 ||
      !Number.isFinite(item.constructionNanoseconds) || item.constructionNanoseconds < 0 || !Number.isInteger(item.vertices) || item.vertices <= 0 ||
      !Number.isInteger(item.strands) || item.strands < 0) throw new Error(`${label}: malformed workload ${i}`)
  }
  return result
}
function median(values) {
  const sorted = [...values].sort((a, b) => a - b), n = sorted.length
  return n % 2 ? sorted[(n - 1) / 2] : (sorted[n / 2 - 1] + sorted[n / 2]) / 2
}
function p95(values) { const sorted = [...values].sort((a, b) => a - b); return sorted[Math.ceil(sorted.length * 0.95) - 1] }
async function main() {
  const options = parseOptions()
  const requestedOutput = options.get('--output')
  try {
    if ((await readdir(requestedOutput)).length !== 0) throw new Error('output must be a new or empty directory')
  } catch (error) {
    if (error.code !== 'ENOENT') throw error
    await mkdir(requestedOutput, { recursive: true })
  }
  output = requestedOutput
  const baseline = options.get('--baseline-source')
  if (baseline && !(await stat(baseline)).isDirectory()) throw new Error('--baseline-source must be a directory')
  manifest.candidateSource = root
  manifest.baselineSource = baseline ?? null
  manifest.startedAt = new Date().toISOString()
  manifest.architecture = process.arch
  manifest.platform = process.platform
  manifest.node = process.version
  const generated = join(output, 'generated', 'RigLimits.swift')
  await writeRigLimits(generated)
  await hash(generated, 'generated-limits-swift')
  await hash(join(dirname(generated), 'RigLimits.rs'), 'generated-limits-rust')
  await hash(join(root, 'native/rig/limits.json'), 'limits-input')
  await hash(join(root, 'native/rig/build-limits.mjs'), 'limits-generator')
  await hash(fileURLToPath(import.meta.url), 'runner')
  const harness = join(root, 'tools/rig-deformation-check/main.swift')
  await hash(harness, 'shared-harness')
  const compiler = (await run('find-swiftc', 'xcrun', ['--find', 'swiftc'])).trim()
  const sdk = (await run('find-sdk', 'xcrun', ['--sdk', 'macosx', '--show-sdk-path'])).trim()
  if (!isAbsolute(compiler) || !isAbsolute(sdk)) throw new Error('xcrun returned invalid compiler or SDK path')
  manifest.compiler = compiler; manifest.sdk = sdk
  await hash(compiler, 'compiler')
  await run('compiler-version', compiler, ['--version'])
  await run('sdk-version', 'xcrun', ['--sdk', 'macosx', '--show-sdk-version'])
  await hash(join(sdk, 'SDKSettings.json'), 'sdk-settings')
  async function compile(source, label) {
    const bridge = join(source, 'native/rig/RigBridge.h')
    const sources = sourceNames.map(name => join(source, 'native/rig', name))
    await hash(bridge, `${label}-bridge`)
    for (const path of sources) await hash(path, `${label}-source`)
    const binary = join(output, `${label}-probe`)
    await run(`compile-${label}`, compiler, ['-sdk', sdk, '-target', 'arm64-apple-macos13.0', '-O', '-whole-module-optimization',
      '-import-objc-header', bridge, ...sources, generated, harness, '-framework', 'Foundation', '-o', binary])
    await hash(binary, `${label}-binary`)
    return binary
  }
  const candidateBinary = await compile(root, 'candidate')
  await run('candidate-check', candidateBinary, ['check'])
  if (!baseline) {
    manifest.result = 'candidate regression checks passed; no baseline supplied; no performance improvement claim'
    await saveManifest()
    console.log(manifest.result)
    return
  }
  const baselineBinary = await compile(baseline, 'baseline')
  const oracle = join(output, 'baseline-oracle.json')
  await run('baseline-oracle', baselineBinary, ['oracle', oracle])
  await hash(oracle, 'baseline-oracle')
  await run('candidate-compare', candidateBinary, ['compare', oracle])
  const pairs = []
  for (let repetition = 1; repetition <= 12; repetition++) {
    const pair = { repetition, order: repetition % 2 ? ['baseline', 'candidate'] : ['candidate', 'baseline'] }
    for (const label of pair.order) {
      const binary = label === 'baseline' ? baselineBinary : candidateBinary
      pair[label] = parseBenchmark(await run(`benchmark-${repetition}-${label}`, binary, ['benchmark']), label)
    }
    for (let i = 0; i < 4; i++) {
      const b = pair.baseline[i], c = pair.candidate[i]
      if (b.checksum !== c.checksum || b.vertices !== c.vertices || b.strands !== c.strands) throw new Error(`benchmark repetition ${repetition} workload ${b.workload}: checksum/workload mismatch`)
    }
    pair.hairRatio = pair.candidate.slice(1).reduce((s, x) => s + x.nsPerUpdate, 0) / pair.baseline.slice(1).reduce((s, x) => s + x.nsPerUpdate, 0)
    pairs.push(pair)
    await writeFile(join(output, 'benchmark-pairs.json'), JSON.stringify(pairs, null, 2) + '\n')
  }
  const workloads = {}
  for (let i = 0; i < 4; i++) {
    const baselineSamples = pairs.map(p => p.baseline[i]), candidateSamples = pairs.map(p => p.candidate[i])
    const summary = samples => ({ medianNsPerUpdate: median(samples.map(x => x.nsPerUpdate)), p95BatchNanoseconds: p95(samples.map(x => x.batchNanoseconds)),
      medianConstructionNanoseconds: median(samples.map(x => x.constructionNanoseconds)), vertices: samples[0].vertices, strands: samples[0].strands })
    workloads[baselineSamples[0].workload] = { baseline: summary(baselineSamples), candidate: summary(candidateSamples),
      medianPairedRatio: median(pairs.map(p => p.candidate[i].nsPerUpdate / p.baseline[i].nsPerUpdate)) }
  }
  const hairMedianRatio = median(pairs.map(p => p.hairRatio)), hairFasterPairs = pairs.filter(p => p.hairRatio < 1).length
  const mixedRatio = workloads.mixed.candidate.medianNsPerUpdate / workloads.mixed.baseline.medianNsPerUpdate
  const passed = hairMedianRatio < 1 && hairFasterPairs >= 9 && mixedRatio <= 1.05
  const summary = { repetitions: 12, workloads, hairMedianRatio, hairFasterPairs, mixedRatio, passed,
    cachePayload: { maximumPowersBytesPerModel: 24 * 32768, maximumNetAdditionalBytesPerModel: 20 * 32768,
      maximumNetAdditionalBytesBaseAndPose: 40 * 32768, note: 'Payload bound only; excludes array headers and allocator overhead. Process peak RSS is measured separately.' } }
  await writeFile(join(output, 'benchmark-summary.json'), JSON.stringify(summary, null, 2) + '\n')
  // These untimed evidence runs intentionally follow, never overlap, the paired protocol.
  await run('baseline-process-memory', '/usr/bin/time', ['-l', baselineBinary, 'benchmark'])
  await run('candidate-process-memory', '/usr/bin/time', ['-l', candidateBinary, 'benchmark'])
  manifest.performanceClaim = passed
  manifest.result = passed ? 'bit-for-bit comparison and paired performance gates passed' : 'performance gates failed; inspect background load and repeat identical protocol once in a fresh output directory; do not claim improvement'
  manifest.finishedAt = new Date().toISOString()
  await saveManifest()
  console.log(JSON.stringify(summary, null, 2))
  if (!passed) throw new Error(manifest.result)
}
main().catch(async error => {
  manifest.error = error.stack ?? String(error)
  await saveManifest().catch(() => {})
  console.error(`check-rig-deformation: ${error.message}`)
  process.exitCode = error.exitStatus ?? 1
})
