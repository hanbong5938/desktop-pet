import AppKit
import Foundation
import ImageIO
import Metal

struct CheckFailure: Error, CustomStringConvertible {
    let description: String
    init(_ message: String) { description = message }
}
func require(_ condition: Bool, _ message: String) throws {
    if !condition { throw CheckFailure(message) }
}
func pump(_ seconds: TimeInterval) {
    let deadline = ProcessInfo.processInfo.systemUptime + seconds
    repeat {
        _ = RunLoop.main.run(mode: .default, before: Date(timeIntervalSinceNow: 0.002))
    } while ProcessInfo.processInfo.systemUptime < deadline
}
func until(_ label: String, timeout: TimeInterval = 10, _ predicate: () throws -> Bool) throws {
    let deadline = ProcessInfo.processInfo.systemUptime + timeout
    repeat {
        if try predicate() { return }
        pump(0.002)
    } while ProcessInfo.processInfo.systemUptime < deadline
    throw CheckFailure("timeout: \(label); a usable AppKit/WindowServer/Metal drawable environment is required")
}
func options() throws -> [String: String] {
    let args = Array(CommandLine.arguments.dropFirst())
    let known: Set<String> = ["--assets", "--expect", "--output"]
    var result: [String: String] = [:]
    try require(args.count == 6, "required: --assets ABS --expect EXPECTATION --output ABS")
    for index in stride(from: 0, to: args.count, by: 2) {
        let key = args[index], value = args[index + 1]
        try require(known.contains(key) && result[key] == nil && !value.hasPrefix("--"), "unknown/duplicate/missing option \(key)")
        result[key] = value
    }
    for key in ["--assets", "--output"] {
        try require((result[key]! as NSString).isAbsolutePath, "\(key) must be absolute")
    }
    try require(["existing-hidden-render", "hidden-stopped"].contains(result["--expect"]!), "invalid expectation")
    return result
}

struct Manifest: Decodable {
    let format: String
    let version: Int
    let render_mode: String
    let width: UInt32
    let height: UInt32
    let entry: String
}
struct Entry: Decodable {
    struct Model: Decodable { let id: String; let file: String; let overrides: String; let motion: String }
    let version: Int
    let initial: String
    let models: [Model]
    let bindings: [String: String]
}
let semantics = ["waiting", "writing", "failed", "cancelled", "disconnected", "bored", "happy", "head-tap", "torso-tap", "head-pet"]
struct Fixture {
    struct Model { let id: String; let file: URL; let overrides: URL; let motion: URL }
    let manifest: Manifest
    let models: [Model]
    let initial: Int
    let bindings: [UInt8]

    init(_ root: URL) throws {
        let root = root.resolvingSymlinksInPath().standardizedFileURL
        func boundedFile(_ path: String) throws -> URL {
            try require(!path.isEmpty && !(path as NSString).isAbsolutePath, "fixture path must be relative: \(path)")
            let url = root.appendingPathComponent(path).resolvingSymlinksInPath().standardizedFileURL
            try require(url.path.hasPrefix(root.path + "/"), "fixture path escapes pack: \(path)")
            let values = try url.resourceValues(forKeys: [.isRegularFileKey])
            try require(values.isRegularFile == true, "missing regular fixture file: \(path)")
            return url
        }
        let decoder = JSONDecoder()
        let manifest = try decoder.decode(Manifest.self, from: Data(contentsOf: boundedFile("manifest.json")))
        try require(manifest.format == "herdr.character" && manifest.version == 5 && manifest.render_mode == "rig", "fixture must be a version-5 rig pack")
        try require(manifest.width > 0 && manifest.height > 0, "invalid fixture canvas")
        let entry = try decoder.decode(Entry.self, from: Data(contentsOf: boundedFile(manifest.entry)))
        try require(entry.version == 2, "fixture entry version must be 2")
        try require((10...16).contains(entry.models.count), "fixture model count outside host catalog range")
        try require(Set(entry.models.map(\.id)).count == entry.models.count, "duplicate fixture model ID")
        let models = try entry.models.map { model -> Model in
            try require(!model.id.isEmpty, "empty fixture model ID")
            let file = try boundedFile(model.file), overrides = try boundedFile(model.overrides), motion = try boundedFile(model.motion)
            // Reject malformed JSON before creating a host or spawning its real decoder.
            _ = try JSONSerialization.jsonObject(with: Data(contentsOf: overrides))
            _ = try JSONSerialization.jsonObject(with: Data(contentsOf: motion))
            return Model(id: model.id, file: file, overrides: overrides, motion: motion)
        }
        guard let initial = models.firstIndex(where: { $0.id == entry.initial }) else { throw CheckFailure("initial fixture ID is absent") }
        let bindings = try semantics.map { key -> UInt8 in
            guard let id = entry.bindings[key], let index = models.firstIndex(where: { $0.id == id }) else {
                throw CheckFailure("fixture binding missing: \(key)")
            }
            return UInt8(index)
        }
        try require(Set(bindings).count == bindings.count, "fixture bindings must be distinct")
        try require(Int(bindings[0]) == initial, "fixture initial must match waiting binding")
        self.manifest = manifest; self.models = models; self.initial = initial; self.bindings = bindings
    }

    func makeHost(catalog: Bool) throws -> RigNativeHost {
        var strings: [UnsafeMutablePointer<CChar>] = []
        defer { for pointer in strings { free(pointer) } }
        func cString(_ value: String) throws -> UnsafePointer<CChar> {
            guard let pointer = strdup(value) else { throw CheckFailure("C string allocation failed") }
            strings.append(pointer)
            return UnsafePointer(pointer)
        }
        let initial = models[self.initial]
        var asset = HerdrRigAssetInput()
        asset.width = manifest.width; asset.height = manifest.height
        asset.base_path = try cString(initial.file.path)
        asset.overrides_path = try cString(initial.overrides.path)
        asset.base_pose_id = try cString(initial.id)
        asset.initial_pose_kind = catalog ? 0 : -1
        var token = HerdrRigTokenInput()
        token.operation_id = try cString("visibility-check")
        token.reference_id = try cString("fixture")
        token.content_digest = try cString("visibility-fixture")
        token.reference_revision = 1; token.backend_epoch = 1
        let inputs = try models.map { model -> HerdrRigModelInput in
            var input = HerdrRigModelInput()
            input.id = try cString(model.id); input.file_path = try cString(model.file.path)
            input.overrides_path = try cString(model.overrides.path); input.motion_path = try cString(model.motion.path)
            return input
        }
        let motion = [UInt8](try Data(contentsOf: initial.motion))
        return try motion.withUnsafeBufferPointer { bytes in
            asset.initial_motion_bytes = bytes.baseAddress; asset.initial_motion_length = bytes.count
            return try inputs.withUnsafeBufferPointer { rawModels in
                try bindings.withUnsafeBufferPointer { rawBindings in
                    if catalog {
                        asset.models = rawModels.baseAddress; asset.model_count = UInt32(rawModels.count)
                        asset.bindings = rawBindings.baseAddress
                    }
                    return try RigNativeHost(asset: asset, token: token)
                }
            }
        }
    }
}

final class Session {
    let host: RigNativeHost
    let window: NSWindow
    var intent = HerdrRigIntent()
    var epoch: UInt64 = 1
    init(fixture: Fixture, catalog: Bool = false) throws {
        host = try fixture.makeHost(catalog: catalog)
        window = NSWindow(contentRect: NSRect(x: 100, y: 100, width: 512, height: 512),
                          styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.backgroundColor = .clear
        window.isOpaque = false
        window.contentView!.addSubview(host.view)
        host.view.frame = NSRect(x: 0, y: 0, width: 512, height: 512)
        window.orderFront(nil)
        intent.now_seconds = 1; intent.phase_age_seconds = 1
        intent.phase = 0; intent.effect = -1
        intent.pointer_x = .nan; intent.pointer_y = .nan
        intent.pose_kind = catalog ? 0 : -1
    }
    deinit { host.cancel(); window.orderOut(nil); window.close() }
    func readyState() throws -> Bool {
        let state = host.poll(cancelRequested: false)
        if state < 0 { throw CheckFailure("host failed: \(host.errorDescription() ?? "unknown error")") }
        try require(state != 2, "host unexpectedly cancelled")
        return state == 1
    }
    func prepare() throws -> Bool {
        try host.prepareSurface(intent, width: 512, height: 512, backingScale: 1, viewportEpoch: epoch)
    }
    func advance() {
        intent.now_seconds += 0.02; intent.phase_age_seconds += 0.02; intent.pose_age_seconds += 0.02
    }
    func bootstrap() throws {
        try until("production host decode", timeout: TimeInterval(RigLimits.decodeSeconds)) { try readyState() }
        try until("inactive-hidden bootstrap surface") { try prepare() }
        try host.activate()
        host.setVisible(true)
        try until("first visible input surface") { try require(readyState(), "host left ready state"); return host.inputReady() }
        host.setVisible(false)
    }
}

var outputURL: URL?
var results: [[String: Any]] = []
func record(_ name: String, _ details: [String: Any] = [:]) throws {
    var result = details
    result["case"] = name; result["status"] = "passed"
    results.append(result)
    if let outputURL {
        let data = try JSONSerialization.data(withJSONObject: results, options: [.prettyPrinted, .sortedKeys])
        try data.write(to: outputURL.appendingPathComponent("cases.json"), options: .atomic)
    }
}

#if RIG_HOST_VISIBILITY_CHECKS
func snapshotJSON(_ value: RigHostVisibilitySnapshot) throws -> Any {
    try JSONSerialization.jsonObject(with: JSONEncoder().encode(value))
}
func snapshot(_ session: Session) throws -> RigHostVisibilitySnapshot {
    let value = session.host.probeVisibilitySnapshot()
    if let failure = value.gateFailure { throw CheckFailure(failure) }
    return value
}
func drain(_ session: Session) throws {
    var drained = false
    session.host.probeDrainRenderQueue { drained = true }
    try until("render queue drain") { drained }
    _ = try snapshot(session)
}
func hidden(_ session: Session) throws {
    let state = try snapshot(session)
    try require(!state.visible && !state.driverRunning && !state.renderPending && !state.surfaceReady && state.hitsDisabled,
                "hidden host published surface, enabled hits, retained a request, or left driver running")
    try require(session.host.inputEpoch() == 0 && !session.host.inputReady(), "hidden input remained enabled")
    try require(session.host.hit(normalizedX: 0.5, normalizedY: 0.5) == nil, "hidden hit remained available")
}
func sameSurfaceCounts(_ a: RigHostVisibilitySnapshot, _ b: RigHostVisibilitySnapshot) throws {
    try require(a.drawableAttempts == b.drawableAttempts && a.drawStarts == b.drawStarts && a.drawCompletions == b.drawCompletions,
                "hidden surface counters changed: \(a.drawableAttempts)/\(a.drawStarts)/\(a.drawCompletions) -> \(b.drawableAttempts)/\(b.drawStarts)/\(b.drawCompletions)")
}
func show(_ session: Session) throws {
    session.intent.visible = 1
    session.host.setVisible(true)
    try until("fresh visible surface at viewport \(session.epoch)") {
        _ = try session.prepare()
        let value = try snapshot(session)
        return value.viewportEpoch == session.epoch && value.surfaceReady && !value.hitsDisabled
            && value.surfaceReadySerial == value.intentSerial && value.surfaceReadyViewportEpoch == session.epoch
            && session.host.inputReady() && session.host.inputEpoch() != 0
    }
}
func hide(_ session: Session) throws {
    session.intent.visible = 0
    session.host.setVisible(false)
    try drain(session)
    try hidden(session)
}
func race(_ session: Session, stage: RigHostVisibilityGate, label: String) throws {
    try hide(session)
    let before = try snapshot(session)
    let release = DispatchSemaphore(value: 0)
    var reached = false
    session.host.probeGateNextRender(stage, reached: { reached = true }, release: release)
    // Arm before show so the real timer and prepare race cannot evade the handshake.
    session.host.setVisible(true)
    session.intent.visible = 1
    defer { release.signal() }
    try until("gate \(label)") { reached }
    let handshake = try snapshot(session)
    switch stage {
    case .beforeValidation:
        try sameSurfaceCounts(before, handshake)
    case .afterDrawable:
        try require(handshake.drawableAttempts == before.drawableAttempts + 1 && handshake.drawStarts == before.drawStarts,
                    "afterDrawable gate was not reached before draw")
    case .beforePublication:
        try require(handshake.drawStarts == before.drawStarts + 1 && handshake.drawCompletions == before.drawCompletions + 1,
                    "beforePublication gate did not follow a completed draw")
    }
    session.host.setVisible(false)
    session.intent.visible = 0
    release.signal()
    try drain(session)
    let after = try snapshot(session)
    try hidden(session)
    try sameSurfaceCounts(handshake, after)
    try require(!after.renderPending, "cancelled gated request retained renderPending")
    try record(label, ["before": try snapshotJSON(before), "handshake": try snapshotJSON(handshake), "after": try snapshotJSON(after)])
}
func candidate(_ fixture: Fixture) throws {
    let session = try Session(fixture: fixture)
    try session.bootstrap()
    try drain(session)
    try record("inactive-hidden-bootstrap", ["snapshot": try snapshotJSON(snapshot(session))])
    let baseline = try snapshot(session)
    for index in 0..<30 {
        if index % 3 == 1 { session.advance(); session.intent.phase = Int32((index / 3) % 3) }
        if index % 3 == 2 { session.epoch += 1 }
        try require(!session.prepare(), "active-hidden prepare returned true at call \(index)")
        session.host.setVisible(false)
        pump(0.5 / 30)
    }
    try drain(session)
    try hidden(session)
    let stopped = try snapshot(session)
    try sameSurfaceCounts(baseline, stopped)
    let serial = stopped.intentSerial
    for _ in 0..<10 { session.host.setVisible(false) }
    try require(try snapshot(session).intentSerial == serial, "duplicate hide changed intentSerial")
    try record("hidden-30-prepares-500ms", ["before": try snapshotJSON(baseline), "after": try snapshotJSON(stopped)])
    try race(session, stage: .beforeValidation, label: "hide-before-validation")
    try race(session, stage: .afterDrawable, label: "hide-after-drawable")
    try race(session, stage: .beforePublication, label: "hide-before-publication")
    session.advance(); session.intent.phase = 2; session.epoch += 1
    try require(!session.prepare(), "hidden latest intent unexpectedly rendered")
    let beforeShow = try snapshot(session)
    try show(session)
    let shown = try snapshot(session)
    try require(shown.drawCompletions > beforeShow.drawCompletions && shown.driverStarts == beforeShow.driverStarts + 1,
                "show did not produce current viewport draw and one driver")
    for _ in 0..<10 { session.host.setVisible(true) }
    try require(try snapshot(session).driverStarts == shown.driverStarts, "duplicate show restarted driver")
    try record("latest-intent-viewport-resume", ["hidden": try snapshotJSON(beforeShow), "shown": try snapshotJSON(shown)])
    for _ in 0..<10 {
        try hide(session)
        let before = try snapshot(session)
        session.advance()
        try show(session)
        try require(try snapshot(session).driverStarts == before.driverStarts + 1, "visibility cycle did not create exactly one driver")
    }
    try hide(session)
    let finalHidden = try snapshot(session)
    pump(0.5)
    try drain(session)
    try sameSurfaceCounts(finalHidden, snapshot(session))
    try record("ten-hide-show-cycles", ["snapshot": try snapshotJSON(snapshot(session))])
    let beforePreview = try snapshot(session)
    let png = try session.host.previewPNG(session.intent, hitOverlay: false)
    guard let source = CGImageSourceCreateWithData(png as CFData, nil),
          let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else { throw CheckFailure("preview PNG did not decode") }
    try require(image.width == Int(fixture.manifest.width) && image.height == Int(fixture.manifest.height), "preview canvas differs from manifest")
    try png.write(to: outputURL!.appendingPathComponent("hidden-preview.png"))
    try hidden(session)
    try sameSurfaceCounts(beforePreview, snapshot(session))
    try require(try snapshot(session).driverStarts == beforePreview.driverStarts, "preview started display driver")
    try record("hidden-preview", ["width": image.width, "height": image.height])
    session.host.cancel()
    let cancelled = try snapshot(session)
    session.host.setVisible(true)
    var rejected = false
    do { _ = try session.prepare() } catch RigNativeError.cancelled { rejected = true }
    try require(rejected, "cancelled prepare did not throw cancellation")
    pump(0.1)
    try drain(session)
    try sameSurfaceCounts(cancelled, snapshot(session))
    try require(!session.host.inputReady() && !(try snapshot(session)).driverRunning, "cancelled show resumed driver/input")
    try record("hidden-cancel-is-terminal")

    do {
        let recovery = try Session(fixture: fixture)
        try recovery.bootstrap()
        try drain(recovery)
        try show(recovery)
        recovery.host.probeTriggerRecovery()
        recovery.host.setVisible(false)
        recovery.intent.visible = 0
        try until("bounded recovery ready", timeout: TimeInterval(RigLimits.decodeSeconds)) { try recovery.readyState() }
        try drain(recovery)
        try hidden(recovery)
        let rebuilt = try snapshot(recovery)
        pump(0.5)
        try drain(recovery)
        try sameSurfaceCounts(rebuilt, snapshot(recovery))
        try show(recovery)
        try require(try snapshot(recovery).drawCompletions > rebuilt.drawCompletions, "recovery failed to resume real draw")
        try record("hidden-during-bounded-recovery", ["rebuilt": try snapshotJSON(rebuilt), "resumed": try snapshotJSON(snapshot(recovery))])
        try hide(recovery)
    }
    do {
        let catalog = try Session(fixture: fixture, catalog: true)
        try catalog.bootstrap()
        try drain(catalog)
        let before = try snapshot(catalog)
        catalog.advance(); catalog.intent.pose_kind = 1; catalog.intent.pose_age_seconds = 0
        try catalog.host.update(catalog.intent)
        let expected = Int(fixture.bindings[1])
        try until("hidden catalog model install", timeout: TimeInterval(RigLimits.decodeSeconds)) {
            _ = try catalog.readyState()
            if let error = catalog.host.errorDescription() { throw CheckFailure("catalog model switch: \(error)") }
            let current = try snapshot(catalog)
            return current.selectedModelIndex == expected && current.pendingModelIndex == nil
        }
        try drain(catalog)
        try hidden(catalog)
        let installed = try snapshot(catalog)
        try sameSurfaceCounts(before, installed)
        try require(installed.driverStarts == before.driverStarts, "hidden catalog install started driver")
        try show(catalog)
        let resumed = try snapshot(catalog)
        try require(resumed.selectedModelIndex == expected && resumed.drawCompletions > installed.drawCompletions,
                    "show did not render newly selected catalog model")
        try record("hidden-catalog-model-switch", ["before": try snapshotJSON(before), "installed": try snapshotJSON(installed), "resumed": try snapshotJSON(resumed)])
        try hide(catalog)
    }
}
#endif

func main() throws {
    let args = try options()
    let output = URL(fileURLWithPath: args["--output"]!, isDirectory: true)
    var isDirectory: ObjCBool = false
    if FileManager.default.fileExists(atPath: output.path, isDirectory: &isDirectory) {
        try require(isDirectory.boolValue && (try FileManager.default.contentsOfDirectory(atPath: output.path)).isEmpty,
                    "results output must be a new or empty directory")
    } else { try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true) }
    outputURL = output
    let fixture = try Fixture(URL(fileURLWithPath: args["--assets"]!, isDirectory: true))
    _ = NSApplication.shared
    NSApp.setActivationPolicy(.accessory)
    NSApp.finishLaunching()
    try require(!NSScreen.screens.isEmpty, "environment blocker: no AppKit screen/WindowServer")
    try require(MTLCreateSystemDefaultDevice() != nil, "environment blocker: no Metal device")
    if args["--expect"] == "existing-hidden-render" {
        let session = try Session(fixture: fixture)
        try session.bootstrap()
        try until("baseline active-hidden render reproduction") { try session.prepare() }
        try require(session.host.inputEpoch() == 0 && !session.host.inputReady(), "baseline hidden input unexpectedly enabled")
        try record("existing-hidden-render-reproduced", ["activeHiddenPrepareReturnedTrue": true])
    } else {
#if RIG_HOST_VISIBILITY_CHECKS
        try candidate(fixture)
#else
        throw CheckFailure("hidden-stopped requires RIG_HOST_VISIBILITY_CHECKS")
#endif
    }
    let summary: [String: Any] = ["status": "passed", "expectation": args["--expect"]!, "cases": results.count]
    try JSONSerialization.data(withJSONObject: summary, options: [.prettyPrinted, .sortedKeys])
        .write(to: output.appendingPathComponent("summary.json"))
    print(String(data: try JSONSerialization.data(withJSONObject: summary, options: [.sortedKeys]), encoding: .utf8)!)
}

do { try main() } catch {
    let message = "rig-host-visibility-check: \(error)"
    FileHandle.standardError.write(Data((message + "\n").utf8))
    if let outputURL, let data = try? JSONSerialization.data(withJSONObject: ["status": "failed", "error": message, "completedCases": results], options: [.prettyPrinted, .sortedKeys]) {
        try? data.write(to: outputURL.appendingPathComponent("failure.json"))
    }
    exit(1)
}
