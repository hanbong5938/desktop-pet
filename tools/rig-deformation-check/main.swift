import Foundation
import simd
import Darwin

private enum CheckError: Error, CustomStringConvertible {
    case failed(String)
    var description: String { switch self { case .failed(let message): return message } }
}
private func require(_ condition: @autoclosure () -> Bool, _ message: String) throws {
    if !condition() { throw CheckError.failed(message) }
}
private struct Fixture {
    let name: String
    let rig: RigDefinition
    let rgba: Data
    var physics: RigHairPhysicsConfig? = nil
    func make() throws -> RigDeformer { try RigDeformer(rig: rig, physics: physics) }
}
private final class Builder {
    var layers: [RigLayer] = []
    var rgba = Data()
    func layer(_ name: String, bounds: [Double] = [32,16,192,144], group: String = "head", side: String? = nil,
               fade: String? = nil, expression: String? = nil, source: String? = nil, parent: String? = nil,
               strands: [RigHairStrand]? = nil, attachment: Bool = false, follow: Bool = false, z: Double? = nil, hairPhysics: Bool = false) {
        let index = layers.count, offset = rgba.count
        for y in 0..<8 { for x in 0..<8 {
            rgba.append(contentsOf: [UInt8((index * 47 + 31) % 256), UInt8((index * 71 + 63) % 256), UInt8((index * 97 + 127) % 256),
                x == 0 || y == 0 || x == 7 || y == 7 ? 0 : (x == 4 ? 128 : 255)])
        } }
        layers.append(RigLayer(name: name, x: bounds[0], y: bounds[1], w: bounds[2], h: bounds[3],
            z: z ?? Double(index), depth: 0.6, group: group, phys: strands != nil || hairPhysics ? "hair" : nil, fade: fade,
            mouthExpression: expression, deformationSource: source, meshSource: parent,
            hairAttachment: attachment ? RigHairAttachment(rootY: 30, bodyY: 180) : nil,
            headFollow: follow ? RigHeadFollow(center: RigPoint(cx: 128, cy: 88), radius: 24, falloffRadius: 120) : nil,
            side: side, strands: strands, synthetic: nil, img: RigImage(width: 8, height: 8, offset: offset, length: 256)))
    }
    func fixture(_ name: String, profile: Bool = false, closed: Bool = false, physics: RigHairPhysicsConfig? = nil) -> Fixture {
        func eye(_ cx: Double) -> RigEyeAnchor {
            let center = RigPoint(cx: cx, cy: 64)
            let blink = profile ? RigEyeBlinkProfile(center: center, angleDeg: 13, u0: -24, u1: 24,
                upper: [-4,-8,-4], lower: [4,8,4], closed: [0,1,0], closedSource: center,
                closedTarget: RigPoint(cx: cx, cy: 68), closedRotationDeg: -7) : nil
            return RigEyeAnchor(x0: cx-24, y0: 48, x1: cx+24, y1: 80, icx: cx, icy: 64, closeY: 68, blink: blink)
        }
        let neutral = RigMouthShape(u0: -32, u1: 32, upper: closed ? [0,0,0] : [-1,-2,-1], lower: closed ? [0,0,0] : [1,2,1])
        let smile = RigMouthShape(u0: -32, u1: 32, upper: closed ? [0,0,0] : [0,2,0], lower: closed ? [0,0,0] : [1,4,1])
        let morph = profile ? RigMouthMorphProfile(center: RigPoint(cx: 128, cy: 124), angleDeg: -9, neutral: neutral,
            open: RigMouthShape(u0: -32, u1: 32, upper: [-2,-4,-2], lower: [5,12,5]), smile: smile) : nil
        let anchors = RigAnchors(face: RigFaceBounds(x0: 32, y0: 16, x1: 224, y1: 160, cx: 128, cy: 88),
            eyeL: eye(80), eyeR: eye(176), mouth: RigMouthAnchor(x0: 96, y0: 112, x1: 160, y1: 136, cx: 128, cy: 124, morph: morph),
            neckPivot: RigPoint(cx: 128, cy: 144), bodyPivot: RigPoint(cx: 128, cy: 208), neckTop: 144, neckBottom: 176, hairRootY: 16, faceScale: 1)
        return Fixture(name: name, rig: RigDefinition(canvas: RigCanvas(w: 256, h: 256), layers: layers, anchors: anchors,
            interactionAreas: ["head": RigBounds(x0: 32, y0: 16, x1: 224, y1: 160), "torso": RigBounds(x0: 0, y0: 144, x1: 256, y1: 256)],
            warnings: [], synth: RigSynthesis(eye: false, mouth: false)), rgba: rgba, physics: physics)
    }
}
private let strands = [RigHairStrand(x: 32, rootY: 20, tipY: 210), RigHairStrand(x: 128, rootY: 30, tipY: 245), RigHairStrand(x: 224, rootY: 10, tipY: 225)]
private func physics(_ overrides: [String: [String: Double]]) -> RigHairPhysicsConfig {
    RigHairPhysicsConfig(frontHair: RigHairPhysicsTuning(amplitude: 0.72, stiffness: 78, damping: 12, wind: 0.34, inertia: 0.62, rootLock: 0.3, maxOffset: 14),
        backHair: RigHairPhysicsTuning(amplitude: 0.86, stiffness: 48, damping: 9.5, wind: 0.46, inertia: 0.82, rootLock: 0.24, maxOffset: 22), layers: overrides)
}
private func mixed(_ variant: String = "normal") -> Fixture {
    let b = Builder()
    for (suffix, side, x) in [("l","L",56.0),("r","R",152.0)] {
        for base in ["eye_close", "irides", "eyebrow"] {
            b.layer(base+"_"+suffix, bounds: [x,48,48,32], group: variant == "group" ? "other" : "head",
                side: variant == "nil-side" ? nil : (variant == "unknown-side" ? "?" : side),
                fade: variant == "nil-fade" ? nil : (variant == "unknown-fade" ? "?" : (base == "eye_close" ? "eyeClose" : (base == "irides" ? "eyeOpen" : nil))))
        }
    }
    b.layer("mouth_open", bounds: [96,112,64,24], fade: "mouthOpen")
    b.layer("mouth_close", bounds: [96,112,64,24], fade: "mouthClose")
    b.layer("face"); b.layer("neck", bounds: [96,144,64,32], group: "body")
    b.layer("topwear", bounds: [48,160,160,80], group: "body")
    b.layer("handwear", bounds: [32,176,32,64], group: "body")
    b.layer("unknown", group: "other", side: "?", fade: "?")
    return b.fixture("mixed-"+variant)
}
private func profiles(_ enabled: Bool, closed: Bool = false) -> Fixture {
    let b = Builder()
    for (suffix, side, x) in [("l","L",56.0),("r","R",152.0)] {
        b.layer("eyewhite_"+suffix, bounds: [x,48,48,32], side: side, fade: "eyeOpen")
        b.layer("irides_"+suffix, bounds: [x,48,48,32], side: side, fade: "eyeOpen")
        b.layer("eye_close_"+suffix, bounds: [x,48,48,32], side: side, fade: "eyeClose")
    }
    for expression in ["neutral","open","smile"] {
        b.layer("mouth_"+expression, bounds: [96,112,64,24], expression: expression)
    }
    return b.fixture("profiles-\(enabled)-closed-\(closed)", profile: enabled, closed: closed)
}
private func hair(_ kind: String, follow: Bool = false, override: Bool = false, source: Bool = false) -> Fixture {
    let b = Builder(), name = kind == "back" ? "back hair" : (kind == "suffix" ? "front hair_1" : "front hair")
    b.layer(name, bounds: [16,8,224,232], strands: strands, attachment: kind == "attached", follow: follow)
    if source { b.layer("custom", bounds: [16,8,224,232], source: "front hair", strands: strands) }
    let config = override ? physics(["front hair": ["rootLock": 0.35,"amplitude": 1.7], "front hair_1": ["rootLock": 0.55,"amplitude": 0.8], "back hair": ["rootLock": 0.35,"amplitude": 1.7]]) : nil
    return b.fixture("hair-\(kind)-follow-\(follow)-override-\(override)-source-\(source)", physics: config)
}
private func parentFixture(cycle: Bool = false) -> Fixture {
    let b = Builder()
    b.layer("child", fade: "eyeOpen", parent: "parent", z: 0)
    b.layer("grandchild", fade: "mouthOpen", parent: "child", z: 1)
    b.layer("sibling", fade: "mouthClose", parent: "parent", z: 2)
    b.layer("parent", parent: cycle ? "grandchild" : nil, z: 3)
    return b.fixture("parent")
}
private func fixtures() -> [Fixture] {
    var result = ["normal","nil-side","unknown-side","nil-fade","unknown-fade","group"].map(mixed)
    result += [profiles(true),profiles(false),profiles(true, closed: true),parentFixture()]
    for kind in ["front","back","attached"] { for follow in [false,true] { for override in [false,true] {
        result.append(hair(kind, follow: follow, override: override))
    } } }
    result += [hair("suffix", override: true),hair("front", source: true)]
    return result
}

private struct MeshRecord: Codable, Equatable {
    let name: String; let nx: Int; let ny: Int
    let indices: [UInt16]; let base: [UInt32]; let uv: [UInt32]; let positions: [UInt32]; let alpha: UInt32
}
private struct HitRecord: Codable, Equatable {
    let x: UInt64; let y: UInt64; let threshold: UInt64
    let region: String?; let layer: String?; let values: [UInt64]
}
private struct Record: Codable, Equatable {
    let caseName: String; let frame: Int; let time: UInt64; let meshes: [MeshRecord]; let hits: [HitRecord]
}
private func bits(_ points: [SIMD2<Float>]) -> [UInt32] { points.flatMap { [$0.x.bitPattern,$0.y.bitPattern] } }
private func hitRecords(_ f: Fixture, _ d: RigDeformer) -> [HitRecord] {
    var queries: [(Double,Double)] = []
    for x in [-8.0,64,128,192,264] { for y in [-8.0,64,128,192,264] { queries.append((x,y)) } }
    queries += [(128,144),(128,160)]
    return [0.1,8.0/255].flatMap { threshold in queries.map { x,y in
        let h = rigSemanticHit(rig: f.rig, meshes: d.meshes, rgba: f.rgba, modelX: x, modelY: y, minimumAlpha: threshold)
        return HitRecord(x: x.bitPattern, y: y.bitPattern, threshold: threshold.bitPattern, region: h?.region, layer: h?.layer,
            values: h.map { [$0.sourceX.bitPattern,$0.sourceY.bitPattern,$0.coverage.bitPattern,$0.renderSlot.bitPattern] } ?? [])
    } }
}
private func snapshot(_ f: Fixture, _ d: RigDeformer, _ name: String, _ frame: Int, _ time: Double, hits: Bool) -> Record {
    Record(caseName: name, frame: frame, time: time.bitPattern, meshes: d.meshes.map {
        MeshRecord(name: $0.layer.name, nx: $0.nx, ny: $0.ny, indices: $0.indices, base: bits($0.base), uv: bits($0.uvs), positions: bits($0.positions), alpha: $0.alpha.bitPattern)
    }, hits: hits ? hitRecords(f,d) : [])
}
private func mismatch(_ expected: Record, _ actual: Record) -> String? {
    let context = "case \(actual.caseName) frame \(actual.frame)"
    if expected.caseName != actual.caseName || expected.frame != actual.frame || expected.time != actual.time { return context+" case/frame/time mismatch" }
    if expected.meshes.count != actual.meshes.count { return context+" mesh count mismatch" }
    for (e,a) in zip(expected.meshes,actual.meshes) {
        let prefix = context+" layer \(a.name)"
        if e.name != a.name || e.nx != a.nx || e.ny != a.ny { return prefix+" name/grid mismatch" }
        for (label, lhs, rhs) in [("base",e.base,a.base),("uv",e.uv,a.uv),("positions",e.positions,a.positions)] {
            if lhs.count != rhs.count { return prefix+" \(label) count mismatch" }
            if let i = lhs.indices.first(where: { lhs[$0] != rhs[$0] }) { return prefix+" vertex \(i/2) component \(i%2) \(label): \(lhs[i]) != \(rhs[i])" }
        }
        if e.indices != a.indices { return prefix+" indices mismatch" }
        if e.alpha != a.alpha { return prefix+" alpha: \(e.alpha) != \(a.alpha)" }
    }
    if expected.hits.count != actual.hits.count { return context+" hit count mismatch" }
    for (e,a) in zip(expected.hits,actual.hits) where e != a { return context+" hit query (\(Double(bitPattern:a.x)),\(Double(bitPattern:a.y))) threshold \(Double(bitPattern:a.threshold)) mismatch: \(e) != \(a)" }
    return nil
}
private func transforms(_ d: RigDeformer, _ mode: Int) -> [RigLocalTransform] {
    d.meshes.enumerated().map { index,mesh in
        var t = RigLocalTransform()
        t.binding = RigPreparedLayerMotion(meshIndex: index, origin: SIMD2(0.27,0.63), weights: mesh.base.indices.map { Float($0 % 3) * 0.5 })
        if mode == 1 || mode == 4 { t.translateX = 3; t.translateY = -4 }
        if mode == 2 || mode == 4 { t.rotationDeg = 17 }
        if mode == 3 || mode == 4 { t.scale = 1.12 }
        return t
    }
}
private func animated(_ seed: RigParameters, _ i: Int) -> RigParameters {
    var p = seed
    p.angleX = 0.4*sin(Double(i)*0.03); p.angleY = -0.3*cos(Double(i)*0.02)
    p.angleZ = 0.2*sin(Double(i)*0.01); p.body = 0.25*cos(Double(i)*0.025)
    return p
}
private func sweeps() -> [(String,RigParameters)] {
    var result: [(String,RigParameters)] = [("default",RigParameters())]
    func sweep(_ name: String, _ values: [Double], _ set: (inout RigParameters,Double)->Void) {
        for value in values { var p = RigParameters(); set(&p,value); result.append(("\(name)=\(value)",p)) }
    }
    sweep("eyeL",[0,0.08,0.11,0.14,0.32,1]) { $0.eyeOpenL = $1 }
    sweep("eyeR",[0,0.08,0.11,0.14,0.32,1]) { $0.eyeOpenR = $1 }
    sweep("iris",[0.5,1.3]) { $0.irisScale = $1; $0.eyeX = 0.78; $0.eyeY = -0.34; $0.eyeScaleL = 0.75; $0.eyeScaleR = 1.25 }
    sweep("mouthOpen",[0,0.2075,0.2675,0.3275,1]) { $0.mouthOpen = $1 }
    sweep("mouthForm",[-1,0,0.62,1]) { $0.mouthForm = $1 }
    sweep("mouthScale",[0.7,1.3]) { $0.mouthScale = $1; $0.mouthCY = 0.16; $0.mouthCAng = -0.4 }
    sweep("fhSoft",[0,0.4,2]) { $0.fhSoft = $1; $0.bangL = 0.8; $0.bangC = -0.3; $0.bangR = 0.5 }
    sweep("soft",[0,2,3]) { $0.soft = $1 }
    sweep("fhAmp",[0,2,4]) { $0.fhAmp = $1 }
    sweep("physAmp",[0,2,4]) { $0.physAmp = $1 }
    var p = RigParameters()
    p.angleX = 0.4; p.angleY = -0.3; p.angleZ = 0.2; p.body = 0.25; p.armY = 0.3; p.armPos = -0.2; p.bust = 2.5; p.bustY = 1
    p.bangL = 0.8; p.bangC = -0.3; p.bangR = 0.5; p.eyeX = 0.78; p.eyeY = -0.34; p.eyeScaleL = 0.75; p.eyeScaleR = 1.25
    p.mouthOpen = 0.3275; p.mouthForm = 0.62; p.mouthCY = 0.16; p.mouthCAng = -0.4
    result.append(("combined",p))
    return result
}
private func visitRecords(_ consume: (Record) throws -> Void) throws {
    for f in fixtures() {
        let d = try f.make()
        for neutral in [false,true] {
            for replay in 0..<2 {
                d.resetSimulation()
                for i in 0...600 {
                    let t = Double(i)*1000/60
                    d.update(parameters: animated(RigParameters(),i), time: t, neutral: neutral)
                    if [0,1,2,17,59,120,600].contains(i) { try consume(snapshot(f,d,"\(f.name)/neutral=\(neutral)/replay=\(replay)/regular",i,t,hits:i == 600)) }
                }
                d.resetSimulation()
                let times: [Double] = [0,16,33,33,20,270,1270]
                for (i,t) in times.enumerated() {
                    d.update(parameters: animated(RigParameters(),i), time:t, neutral:neutral)
                    try consume(snapshot(f,d,"\(f.name)/neutral=\(neutral)/replay=\(replay)/irregular",i,t,hits:i == times.count-1))
                }
            }
            for (label,p) in sweeps() {
                d.resetSimulation()
                for i in 0..<3 { d.update(parameters:p,time:Double(i)*1000/60,neutral:neutral) }
                try consume(snapshot(f,d,"\(f.name)/neutral=\(neutral)/sweep/\(label)",2,1000/30,hits:true))
            }
            for mode in 0...4 {
                d.resetSimulation(); let local = transforms(d,mode)
                d.update(parameters:RigParameters(),time:0,neutral:neutral,localTransforms:local)
                try consume(snapshot(f,d,"\(f.name)/neutral=\(neutral)/local=\(mode)",0,0,hits:true))
            }
            d.resetSimulation()
            for (i,body) in [0.0,0.003,0.004,-0.004].enumerated() {
                var p = RigParameters(); p.body = body
                d.update(parameters:p,time:33,neutral:neutral)
                try consume(snapshot(f,d,"\(f.name)/neutral=\(neutral)/same-time-body",i,33,hits:i == 3))
            }
        }
    }
}

private func check() throws {
    let f = parentFixture(), d = try f.make()
    let first = d.meshes[0]
    for m in d.meshes { try require(m.nx == first.nx && m.ny == first.ny && m.base == first.base && m.indices == first.indices,"parent fixture grids differ") }
    guard let pi = d.meshes.firstIndex(where: {$0.layer.name == "parent"}) else { throw CheckError.failed("parent absent") }
    var local = transforms(d,4)
    for i in local.indices where i != pi { local[i].translateX = 100; local[i].scale = 0.5 }
    var p = RigParameters(); p.eyeOpenL = 0.32; p.eyeOpenR = 0.32; p.mouthOpen = 0.7
    d.update(parameters:p,time:33,neutral:false,localTransforms:local)
    for m in d.meshes { try require(bits(m.positions) == bits(d.meshes[pi].positions),"parent local geometry not inherited by \(m.layer.name)") }
    try require(Set(d.meshes.map {$0.alpha.bitPattern}).count > 1,"child alpha did not remain independent")
    do { _ = try parentFixture(cycle:true).make(); throw CheckError.failed("meshSource cycle accepted") }
    catch let error as CheckError { throw error } catch { try require(String(describing:error).lowercased().contains("cycl"),"wrong cycle error: \(error)") }
    for f in fixtures() {
        let d = try f.make()
        for neutral in [false,true] {
            let regular = (0...600).map { Double($0)*1000/60 }
            for times in [regular,[0.0,16,33,33,20,270,1270]] {
                var expected: [Record] = []
                for replay in 0..<2 {
                    d.resetSimulation()
                    var snapshotIndex = 0
                    for (i,t) in times.enumerated() {
                        d.update(parameters:animated(p,i),time:t,neutral:neutral)
                        if times.count == 7 || [0,1,2,17,59,120,600].contains(i) {
                            let record = snapshot(f,d,f.name,i,t,hits:i == times.count-1)
                            if replay == 0 { expected.append(record) }
                            else if let message = mismatch(expected[snapshotIndex],record) { throw CheckError.failed("reset replay: "+message) }
                            snapshotIndex += 1
                        }
                    }
                }
            }
        }
    }
    let fa = hair("front",override:true)
    var fb = hair("front")
    fb.physics = physics(["front hair": ["rootLock":0.65,"amplitude":0.4]])
    func isolatedReplay(_ f: Fixture) throws -> [Record] {
        let d = try f.make()
        return (0...120).map { i in
            let t = Double(i)*1000/60
            d.update(parameters:animated(p,i),time:t,neutral:false)
            return snapshot(f,d,f.name,i,t,hits:i == 120)
        }
    }
    let expectedA = try isolatedReplay(fa), expectedB = try isolatedReplay(fb)
    let a = try fa.make(), b = try fb.make()
    for i in 0...120 {
        let t = Double(i)*1000/60, input = animated(p,i)
        a.update(parameters:input,time:t,neutral:false); b.update(parameters:input,time:t,neutral:false)
        for (f,d,expected) in [(fa,a,expectedA[i]),(fb,b,expectedB[i])] {
            if let message = mismatch(expected,snapshot(f,d,f.name,i,t,hits:i == 120)) { throw CheckError.failed("physics isolation: "+message) }
        }
    }
    let nilBuilder = Builder(), emptyBuilder = Builder()
    nilBuilder.layer("back hair",bounds:[16,8,224,232],hairPhysics:true)
    emptyBuilder.layer("back hair",bounds:[16,8,224,232],strands:[])
    let fn = nilBuilder.fixture("no-strands"), fe = emptyBuilder.fixture("no-strands"), dn = try fn.make(), de = try fe.make()
    for i in 0...120 {
        let t = Double(i)*1000/60, input = animated(p,i)
        dn.update(parameters:input,time:t,neutral:false); de.update(parameters:input,time:t,neutral:false)
        if let message = mismatch(snapshot(fn,dn,fn.name,i,t,hits:true),snapshot(fe,de,fe.name,i,t,hits:true)) { throw CheckError.failed("nil/empty strands: "+message) }
    }
    let hitBuilder = Builder(); hitBuilder.layer("surface",bounds:[0,0,256,256],group:"other")
    let hf = hitBuilder.fixture("hits"), hd = try hf.make()
    // Initial geometry is rest geometry: semantic-region assertions must not depend on breathing offsets.
    func hit(_ x: Double,_ y: Double) -> RigSemanticHit? { rigSemanticHit(rig:hf.rig,meshes:hd.meshes,rgba:hf.rgba,modelX:x,modelY:y,minimumAlpha:0.1) }
    try require(hit(128,88)?.region == "head","opaque head hit missing")
    try require(hit(128,144)?.region == "head","overlap must prefer head")
    try require(hit(128,160)?.region == "body","torso boundary must map to body")
    try require(hit(1,200) == nil,"transparent border must miss")
    for count in [7,8] {
        let q = Builder()
        q.layer("back hair",bounds:[0,0,76.2,153],expression:"neutral",strands:(0..<count).map { RigHairStrand(x:Double(10*$0),rootY:0,tipY:153) })
        let fixture = q.fixture("quota",profile:true)
        if count == 7 {
            let deformer = try fixture.make()
            try require(deformer.meshes[0].nx == 127 && deformer.meshes[0].ny == 255 && deformer.meshes[0].base.count == 32768,"quota fixture grid must be 127x255/32768 vertices")
        } else {
            do { _ = try fixture.make(); throw CheckError.failed("eight-strand quota accepted") }
            catch let error as CheckError { throw error } catch { try require(String(describing:error).contains("strand influence"),"wrong strand quota error: \(error)") }
        }
    }
    print("{\"check\":\"passed\"}")
}

private struct BenchmarkResult: Codable {
    let workload: String; let nsPerUpdate: Double; let batchNanoseconds: UInt64
    let constructionNanoseconds: UInt64; let vertices: Int; let strands: Int; let checksum: String
}
private func checksum(_ d: RigDeformer) -> String {
    var hash: UInt64 = 14695981039346656037
    func add(_ value: UInt32) { for shift in stride(from:0,to:32,by:8) { hash = (hash ^ UInt64((value >> shift) & 255)) &* 1099511628211 } }
    for m in d.meshes { for p in m.positions { add(p.x.bitPattern); add(p.y.bitPattern) }; add(m.alpha.bitPattern) }
    return String(hash,radix:16)
}
private func benchmark() throws {
    var seed = RigParameters(); seed.bangL = 0.8; seed.bangC = -0.3; seed.bangR = 0.5
    seed.fhAmp = 2; seed.physAmp = 2; seed.fhSoft = 0.4; seed.soft = 2
    let inputs = (0..<2400).map { animated(seed,$0) }
    let times = (0..<2400).map { Double($0)*1000/60 }
    var results: [BenchmarkResult] = []
    for (name,f) in [("mixed",mixed()),("front",hair("front")),("back",hair("back")),("attached",hair("attached"))] {
        let constructStart = DispatchTime.now().uptimeNanoseconds
        let d = try f.make()
        let construction = DispatchTime.now().uptimeNanoseconds-constructStart
        d.resetSimulation()
        for i in 0..<600 { d.update(parameters:inputs[i],time:times[i],neutral:false) }
        let start = DispatchTime.now().uptimeNanoseconds
        for i in 600..<2400 { d.update(parameters:inputs[i],time:times[i],neutral:false) }
        let elapsed = DispatchTime.now().uptimeNanoseconds-start
        results.append(BenchmarkResult(workload:name,nsPerUpdate:Double(elapsed)/1800,batchNanoseconds:elapsed,
            constructionNanoseconds:construction,vertices:d.meshes.reduce(0) {$0+$1.base.count},strands:f.rig.layers.reduce(0) {$0+($1.strands?.count ?? 0)},checksum:checksum(d)))
    }
    print(String(decoding:try JSONEncoder().encode(results),as:UTF8.self))
}

// One JSON array, with one independently decodable record per line. Streaming avoids
// keeping all vertex snapshots in memory; records never use padded Swift raw memory.
private func oracle(_ path: String) throws {
    let fd = open(path,O_WRONLY | O_CREAT | O_EXCL,0o600)
    guard fd >= 0 else { throw CheckError.failed("oracle requires a new writable file: \(path): \(String(cString:strerror(errno)))") }
    let file = FileHandle(fileDescriptor:fd,closeOnDealloc:true)
    defer { try? file.close() }
    try file.write(contentsOf:Data("[\n".utf8))
    let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys]
    var first = true, count = 0
    try visitRecords { record in
        if !first { try file.write(contentsOf:Data(",\n".utf8)) }; first = false
        try file.write(contentsOf:encoder.encode(record)); count += 1
    }
    try file.write(contentsOf:Data("\n]\n".utf8))
    print("{\"oracleRecords\":\(count),\"schema\":1}")
}
private final class OracleReader {
    let file: FileHandle
    var buffer = Data(); var ended = false
    init(_ path: String) throws { file = try FileHandle(forReadingFrom:URL(fileURLWithPath:path)) }
    deinit { try? file.close() }
    func line() throws -> Data? {
        while true {
            if let index = buffer.firstIndex(of:10) { let line = Data(buffer[..<index]); buffer.removeSubrange(...index); return line }
            if ended { if buffer.isEmpty { return nil }; let rest = buffer; buffer.removeAll(); return rest }
            let chunk = try file.read(upToCount:65536) ?? Data()
            if chunk.isEmpty { ended = true } else { buffer.append(chunk) }
        }
    }
}
private func compare(_ path: String) throws {
    let reader = try OracleReader(path)
    let opening = try reader.line()
    try require(opening == Data("[".utf8),"broken oracle: expected opening array")
    var pending = try reader.line(), count = 0
    let decoder = JSONDecoder()
    try visitRecords { actual in
        guard var line = pending, !line.isEmpty else { throw CheckError.failed("oracle truncated at case \(actual.caseName) frame \(actual.frame)") }
        // The comma separating records is on its predecessor's line.
        let hasComma = line.last == 44
        if hasComma { line.removeLast() }
        let expected: Record
        do { expected = try decoder.decode(Record.self,from:line) }
        catch { throw CheckError.failed("broken oracle at record \(count) case \(actual.caseName): \(error)") }
        if let message = mismatch(expected,actual) { throw CheckError.failed(message) }
        pending = try reader.line(); count += 1
        if !hasComma && pending != Data("]".utf8) { throw CheckError.failed("broken oracle: missing separator at record \(count)") }
        if hasComma && pending == Data("]".utf8) { throw CheckError.failed("broken oracle: trailing comma") }
    }
    try require(pending == Data("]".utf8),"oracle has extra records or missing closing array")
    let trailing = try reader.line()
    try require(trailing == nil,"oracle has trailing content")
    print("{\"compare\":\"bit-for-bit passed\",\"records\":\(count)}")
}

do {
    let args = Array(CommandLine.arguments.dropFirst())
    switch args.first {
    case "check" where args.count == 1: try check()
    case "benchmark" where args.count == 1: try benchmark()
    case "oracle" where args.count == 2: try oracle(args[1])
    case "compare" where args.count == 2: try compare(args[1])
    default: throw CheckError.failed("usage: rig-deformation-check check | oracle PATH | compare PATH | benchmark")
    }
} catch {
    FileHandle.standardError.write(Data("rig-deformation-check: \(error)\n".utf8))
    exit(1)
}
