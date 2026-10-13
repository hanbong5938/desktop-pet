#!/usr/bin/ruby
# Embedded verbatim in the materialized formula; never installed inside the signed app.
require "json"
require "digest"
require "securerandom"

module DesktopPetDeploymentRecord
  MAX_RECORD = 2 * 1024 * 1024
  MAX_FILE = 512 * 1024 * 1024
  MAX_TOTAL = 2 * 1024 * 1024 * 1024
  MAX_FILES = 32768
  SIGNED_FILES = %w[MacOS/desktop-pet MacOS/desktop-pet-update-coordinator MacOS/rig-decode-worker Frameworks/libherdr_rig.dylib _CodeSignature/CodeResources].freeze

  def self.require_value(condition, message)
    raise message unless condition
  end

  def self.path_valid?(name)
    name.is_a?(String) && !name.empty? && !name.include?("\0") && !name.include?("\\") &&
      name.split("/", -1).all? { |part| !part.empty? && part != "." && part != ".." } && name.split("/").length <= 32
  end

  def self.validate(record, revision)
    require_value(record.is_a?(Hash) && !record.key?("signing_derivation"), "original must be an underived release record")
    {"version" => 1, "paths_relative_to" => "Contents", "probe_faults" => false,
     "core_revision" => revision, "dylib" => "Frameworks/libherdr_rig.dylib", "worker" => "MacOS/rig-decode-worker",
     "coordinator" => "MacOS/desktop-pet-update-coordinator", "decoder" => "Resources/rig/decoder.js",
     "limits" => "Resources/rig/limits.json", "build_record" => "Resources/rig-native-build.json",
     "install_name" => "@rpath/libherdr_rig.dylib", "packaged_rpath" => "@executable_path/../Frameworks"}.each do |key, value|
      require_value(record[key] == value, "release layout/provenance mismatch: #{key}")
    end
    identity = record["distribution_identity"]
    require_value(identity.is_a?(Hash), "missing distribution identity")
    {"distribution" => "neutral", "app_name" => "DesktopPet.app", "executable" => "desktop-pet",
     "helper" => "desktop-pet-update-coordinator", "display_name" => "Desktop Pet",
     "bundle_id" => "io.github.hanbong5938.desktop-pet", "app_version" => "0.4.0", "protocol" => 4,
     "observation_mode" => 2, "integration_api" => 1, "core_revision" => revision}.each do |key, value|
      require_value(identity[key] == value, "neutral distribution identity mismatch: #{key}")
    end
    require_value(identity["herdr_migration_revision"].nil?, "neutral release carries bridge authority")
    hashes = record["deployed_sha256"]
    require_value(hashes.is_a?(Hash) && !hashes.empty? && hashes.length <= MAX_FILES, "invalid deployed file map")
    hashes.each do |name, digest|
      require_value(path_valid?(name) && digest.is_a?(String) && /\A[0-9a-f]{64}\z/.match(digest), "invalid deployed path/hash")
    end
    required = SIGNED_FILES + %w[Info.plist Resources/default/manifest.json Resources/LICENSE.txt Resources/rig/decoder.js Resources/rig/limits.json Resources/rig/NOTICE.txt Resources/rig/RigLimits.swift Resources/rig/RigLimits.rs Resources/rig-native-build.json]
    require_value((required - hashes.keys).empty?, "release lacks required native/rig resources")
    hashes
  end

  def self.safe_stat(path, directory = false)
    stat = File.lstat(path)
    require_value(stat.uid == Process.euid && (stat.mode & 0022).zero? && !stat.symlink? &&
      (directory ? stat.directory? : stat.file? && stat.nlink == 1), "unsafe installed resource: #{path}")
    stat
  end

  def self.read_owned(path, bound)
    before = safe_stat(path)
    require_value(before.size <= bound, "installed resource exceeds size bound")
    result = nil
    File.open(path, File::RDONLY | File::NOFOLLOW | File::NONBLOCK) do |file|
      opened = file.stat
      require_value([before.dev, before.ino, before.size, before.mtime, before.ctime] ==
        [opened.dev, opened.ino, opened.size, opened.mtime, opened.ctime], "installed resource changed before open")
      result = yield file
      after = safe_stat(path)
      final = file.stat
      proof = ->(stat) { [stat.dev, stat.ino, stat.size, stat.mtime, stat.ctime] }
      require_value(proof.call(before) == proof.call(after) && proof.call(after) == proof.call(final),
        "installed resource changed during read")
    end
    result
  end

  def self.hash_file(path)
    read_owned(path, MAX_FILE) do |file|
      hash = Digest::SHA256.new
      size = 0
      while (chunk = file.read(65536))
        size += chunk.bytesize
        require_value(size <= MAX_FILE, "installed resource grew beyond bound")
        hash.update(chunk)
      end
      hash.hexdigest
    end
  end

  def self.files_under(root, relative = "", files = {}, total = [0])
    safe_stat(root, true)
    Dir.children(root).sort.each do |leaf|
      name = relative.empty? ? leaf : "#{relative}/#{leaf}"
      require_value(path_valid?(name), "invalid installed resource path")
      path = File.join(root, leaf)
      stat = File.lstat(path)
      if stat.directory?
        files_under(path, name, files, total)
      else
        safe_stat(path)
        total[0] += stat.size
        require_value(stat.size <= MAX_FILE && total[0] <= MAX_TOTAL && files.length < MAX_FILES, "installed bundle exceeds bounds")
        files[name] = hash_file(path)
      end
    end
    files
  end

  def self.derive(bundle, expected_sha, revision)
    require_value(bundle.start_with?("/") && File.realpath(bundle) == bundle && File.basename(bundle) == "DesktopPet.app", "expected physical neutral bundle")
    safe_stat(bundle, true)
    parent = File.dirname(bundle)
    safe_stat(parent, true)
    original_path = File.join(parent, "DesktopPet.app.release-rig-native.json")
    raw = read_owned(original_path, MAX_RECORD) { |file| file.read(MAX_RECORD + 1) }
    require_value(raw.bytesize <= MAX_RECORD, "release record exceeds bound")
    original_sha = Digest::SHA256.hexdigest(raw)
    require_value(original_sha == expected_sha, "original raw release record no longer matches authenticated formula resource")
    record = JSON.parse(raw)
    original_hashes = validate(record, revision)
    current_hashes = files_under(File.join(bundle, "Contents"))
    require_value(current_hashes.keys.sort == original_hashes.keys.sort, "Brew signing changed the exact release file closure")
    current_hashes.each do |name, digest|
      require_value(digest == original_hashes[name] || SIGNED_FILES.include?(name), "Brew signing changed resource payload: #{name}")
    end
    record["deployed_sha256"] = current_hashes
    record["signing_derivation"] = {"kind" => "homebrew_ad_hoc", "original_record" => "DesktopPet.app.release-rig-native.json", "original_sha256" => original_sha}
    bytes = JSON.pretty_generate(record) + "\n"
    require_value(bytes.bytesize <= MAX_RECORD, "derived record exceeds bound")
    target = "#{bundle}.rig-native.json"
    safe_stat(target) if File.exist?(target) || File.symlink?(target)
    temporary = File.join(parent, ".desktop-pet-rig-#{SecureRandom.hex(16)}.json")
    begin
      File.open(temporary, File::WRONLY | File::CREAT | File::EXCL, 0444) do |file|
        file.write(bytes)
        file.flush
        file.fsync
      end
      require_value(hash_file(original_path) == original_sha, "original release record changed during derivation")
      File.rename(temporary, target)
      File.open(parent, File::RDONLY) { |directory| directory.fsync }
    ensure
      File.unlink(temporary) if File.exist?(temporary)
    end
  end
end

if $PROGRAM_NAME == __FILE__
  abort "Usage: deployment-record.rb ABS/DesktopPet.app AUTHENTICATED_ORIGINAL_SHA256 PUBLISHED_CORE_REVISION" unless ARGV.length == 3
  begin
    DesktopPetDeploymentRecord.derive(*ARGV)
  rescue StandardError => error
    abort "Desktop Pet signing derivation refused: #{error.message}"
  end
end
