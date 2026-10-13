#!/usr/bin/ruby
# Publisher-side preparation only. This never edits a tap or runs an installer/app.
# Run after the fixed six-asset public neutral release exists, with a new physical
# absolute output directory. A formula is emitted only after all bytes authenticate.
require "json"
require "digest"
require "open3"
require "tmpdir"
require "fileutils"
require "zlib"
require "rubygems/package"
require_relative "deployment-record"

module DesktopPetHomebrewMaterializer
  REPOSITORY = "hanbong5938/desktop-pet".freeze
  API = "https://api.github.com/repos/#{REPOSITORY}".freeze
  BASE = "https://github.com/#{REPOSITORY}/releases/download/v0.4.0".freeze
  ARCHIVE = "DesktopPet-v0.4.0-macos-arm64.tar.gz".freeze
  SIDECAR = "DesktopPet-v0.4.0-rig-native.json".freeze
  INSTALLER = "desktop-pet-install-v0.4.0.sh".freeze
  ASSETS = [ARCHIVE, "#{ARCHIVE}.sha256", "SHA256SUMS", SIDECAR, INSTALLER, "#{INSTALLER}.sha256"].freeze
  MAX_ARCHIVE = 1024 * 1024 * 1024
  MAX_API = 4 * 1024 * 1024
  MAX_RECORD = DesktopPetDeploymentRecord::MAX_RECORD

  def self.check(condition, message)
    DesktopPetDeploymentRecord.require_value(condition, message)
  end

  def self.fetch(url, path, bound)
    # Empty environment/config/netrc, no proxy, no authorization/cookies: public
    # availability is required even when the publisher has gh credentials locally.
    _, error, status = Open3.capture3({"PATH" => "/usr/bin:/bin", "LC_ALL" => "C"},
      "/usr/bin/curl", "-q", "--fail", "--silent", "--show-error", "--location", "--max-redirs", "5",
      "--proto", "=https", "--proto-redir", "=https", "--noproxy", "*", "--netrc-file", "/dev/null",
      "--connect-timeout", "15", "--max-time", "300", "--max-filesize", bound.to_s,
      "--header", "Authorization:", "--header", "Cookie:", "--header", "Accept: application/vnd.github+json",
      "--output", path, url, :unsetenv_others => true)
    check(status.success?, "anonymous fixed-authority fetch failed: #{url}: #{error.strip}")
    check(File.size(path) > 0 && File.size(path) <= bound, "download exceeds bound or is empty: #{url}")
  end

  def self.fetch_json(url, path)
    fetch(url, path, MAX_API)
    result = JSON.parse(File.binread(path))
    check(result.is_a?(Hash), "invalid public API object")
    result
  end

  def self.asset_map(release)
    id = release["id"]
    check(id.is_a?(Integer) && id > 0 && release["url"] == "#{API}/releases/#{id}" &&
      release["html_url"] == "https://github.com/#{REPOSITORY}/releases/tag/v0.4.0" &&
      release["tag_name"] == "v0.4.0" && release["draft"] == false && release["prerelease"] == false,
      "expected actual published stable v0.4.0 from the neutral core authority")
    assets = release["assets"]
    check(assets.is_a?(Array) && assets.length == ASSETS.length && assets.map { |a| a["name"] }.sort == ASSETS.sort,
      "release must contain exactly the official six neutral assets")
    assets.each_with_object({}) do |asset, map|
      name = asset["name"]
      bound = name == ARCHIVE ? MAX_ARCHIVE : MAX_RECORD
      check(asset["state"] == "uploaded" && asset["id"].is_a?(Integer) && asset["id"] > 0 &&
        asset["size"].is_a?(Integer) && asset["size"] > 0 && asset["size"] <= bound &&
        asset["url"] == "#{API}/releases/assets/#{asset['id']}" && asset["browser_download_url"] == "#{BASE}/#{name}",
        "invalid uploaded fixed-authority asset: #{name}")
      map[name] = asset.select { |key, _| %w[name id size state url browser_download_url digest updated_at].include?(key) }
    end
  end

  def self.checksums(path, names)
    rows = File.binread(path).lines
    check(rows.length == names.length, "unexpected checksum entry count")
    rows.each_with_object({}) do |line, hashes|
      match = /\A([0-9a-f]{64})  ([A-Za-z0-9.\-]+)\n?\z/.match(line)
      check(match && names.include?(match[2]) && !hashes.key?(match[2]), "invalid or duplicate checksum entry")
      hashes[match[2]] = match[1]
    end
  end

  def self.revision(evidence)
    object = fetch_json("#{API}/git/ref/tags/v0.4.0", File.join(evidence, "tag.json"))
    check(object["ref"] == "refs/tags/v0.4.0", "tag API returned a different ref")
    object = object["object"]
    hops = 0
    while object.is_a?(Hash) && object["type"] == "tag"
      sha = object["sha"]
      check(sha.is_a?(String) && /\A[0-9a-f]{40}\z/.match(sha) && hops < 4, "invalid or unbounded annotated tag")
      hops += 1
      tag = fetch_json("#{API}/git/tags/#{sha}", File.join(evidence, "tag-#{hops}.json"))
      check(tag["sha"] == sha, "annotated tag identity changed")
      object = tag["object"]
    end
    sha = object.is_a?(Hash) && object["sha"]
    check(object.is_a?(Hash) && object["type"] == "commit" && sha.is_a?(String) &&
      /\A[0-9a-f]{40}\z/.match(sha) && sha != "0" * 40, "missing actual core commit")
    commit = fetch_json("#{API}/git/commits/#{sha}", File.join(evidence, "commit.json"))
    check(commit["sha"] == sha, "core commit API identity mismatch")
    sha
  end

  def self.pax_path(bytes)
    # BSD tar may use per-entry PAX metadata for long resource paths. Never
    # execute/extract entries; honor only the path override and reject link data.
    path = nil
    offset = 0
    while offset < bytes.bytesize
      space = bytes.index(" ", offset)
      check(space && /\A[1-9][0-9]*\z/.match(bytes[offset...space]), "invalid PAX length")
      length = bytes[offset...space].to_i
      row = bytes.byteslice(offset, length)
      check(length > space - offset + 1 && row && row.bytesize == length && row.end_with?("\n"), "invalid PAX record")
      key, value = row.byteslice(space - offset + 1, length - (space - offset + 1) - 1).split("=", 2)
      check(value && !%w[linkpath size GNU.sparse.map GNU.sparse.name].include?(key) && !key.start_with?("GNU.sparse"), "unsupported PAX payload override")
      if key == "path"
        check(path.nil?, "duplicate PAX path override")
        path = value
      end
      offset += length
    end
    path
  end

  def self.validate_archive(path, hashes)
    observed = {}
    seen = {}
    total = 0
    metadata_bytes = 0
    pending_path = nil
    pending_pax = false
    Zlib::GzipReader.open(path) do |gzip|
      Gem::Package::TarReader.new(gzip) do |tar|
        tar.each do |entry|
          if entry.header.typeflag == "x"
            check(!pending_pax && entry.header.size <= MAX_RECORD, "invalid PAX entry chain or size")
            metadata_bytes += entry.header.size
            check(metadata_bytes <= MAX_RECORD, "archive PAX metadata exceeds bound")
            pending_path = pax_path(entry.read)
            pending_pax = true
            next
          end
          name = (pending_path || entry.full_name).sub(%r{/\z}, "")
          pending_path = nil
          pending_pax = false
          check(DesktopPetDeploymentRecord.path_valid?(name) && !seen.key?(name) && seen.length < 65536,
            "archive contains unsafe/duplicate/unbounded path")
          seen[name] = true
          check(name == "DesktopPet.app" || name == "DesktopPet.app/Contents" || name.start_with?("DesktopPet.app/Contents/"),
            "archive contains unrelated payload")
          check((entry.directory? || entry.file?) && entry.header.linkname.to_s.empty? && (entry.header.mode & 0022).zero?,
            "archive contains link/special/writable resource")
          if entry.file?
            check(name.start_with?("DesktopPet.app/Contents/") && entry.header.size <= DesktopPetDeploymentRecord::MAX_FILE &&
              observed.length < DesktopPetDeploymentRecord::MAX_FILES, "archive resource exceeds bounds")
            total += entry.header.size
            check(total <= DesktopPetDeploymentRecord::MAX_TOTAL, "archive unpacked size exceeds bound")
            digest = Digest::SHA256.new
            digest.update(entry.read(65536)) until entry.eof?
            observed[name.delete_prefix("DesktopPet.app/Contents/")] = digest.hexdigest
          end
        end
        check(!pending_pax, "dangling PAX metadata")
      end
      # Force gzip CRC/length verification, also rejecting nonzero trailing tar data.
      padding = 0
      while (extra = gzip.read(65536)) && !extra.empty?
        padding += extra.bytesize
        check(padding <= MAX_RECORD, "archive trailing padding exceeds bound")
        check(extra.bytes.all?(&:zero?), "archive has trailing non-tar payload")
      end
      check((gzip.unused.nil? || gzip.unused.empty?) && gzip.to_io.eof?, "archive has appended gzip payload")
    end
    check(observed == hashes, "authenticated release archive does not match original deployed file closure/hashes")
  end

  def self.formula(archive_sha, sidecar_sha, core_revision)
    helper = File.binread(File.join(__dir__, "deployment-record.rb"))
    # Single-quoted heredoc preserves the embedded helper's own Ruby interpolation.
    embedded = helper.lines.map { |line| "      #{line}" }.join
    text = <<~RUBY
      class DesktopPet < Formula
        desc "Host-neutral native desktop companion with optional agent integrations"
        homepage "https://github.com/#{REPOSITORY}"
        url "#{BASE}/#{ARCHIVE}"
        version "0.4.0"
        sha256 "#{archive_sha}"
        # Software is MIT; bundled artwork is not MIT-licensed.
        license :cannot_represent

        depends_on arch: :arm64
        depends_on macos: :ventura

        resource "release-rig-native" do
          url "#{BASE}/#{SIDECAR}", using: :nounzip
          sha256 "#{sidecar_sha}"
        end

        def install
          # Homebrew stages the sole top-level app by changing into it.
          (libexec/"DesktopPet.app").install "Contents"
          # The app resolves resources relative to its actual image, not this wrapper.
          bin.write_exec_script libexec/"DesktopPet.app/Contents/MacOS/desktop-pet"
          resource("release-rig-native").stage do
            libexec.install "#{SIDECAR}" => "DesktopPet.app.release-rig-native.json"
          end
          chmod 0444, libexec/"DesktopPet.app.release-rig-native.json"
          # Self-contained signing metadata helper, outside the signed app closure.
          (libexec/"derive-rig-native.rb").write <<~'HELPER'
          __EMBEDDED_HELPER__
          HELPER
        end

        post_install_steps do
          # Preserve the deployed formula's packager-matching inner-to-outer order.
          # Worker entitlements are preserved exactly; other code gets no entitlements.
          run "/usr/bin/codesign",
              args: ["--force", "--timestamp=none", "--sign", "-",
                     "{{libexec}}/DesktopPet.app/Contents/Frameworks/libherdr_rig.dylib"],
              writable_paths: ["{{libexec}}/DesktopPet.app"]
          run "/usr/bin/codesign",
              args: ["--force", "--timestamp=none", "--preserve-metadata=entitlements", "--sign", "-",
                     "{{libexec}}/DesktopPet.app/Contents/MacOS/rig-decode-worker"],
              writable_paths: ["{{libexec}}/DesktopPet.app"]
          run "/usr/bin/codesign",
              args: ["--force", "--timestamp=none", "--sign", "-",
                     "{{libexec}}/DesktopPet.app/Contents/MacOS/desktop-pet-update-coordinator"],
              writable_paths: ["{{libexec}}/DesktopPet.app"]
          run "/usr/bin/codesign",
              args: ["--force", "--timestamp=none", "--sign", "-",
                     "{{libexec}}/DesktopPet.app/Contents/MacOS/desktop-pet"],
              writable_paths: ["{{libexec}}/DesktopPet.app"]
          run "/usr/bin/codesign",
              args: ["--force", "--timestamp=none", "--sign", "-", "{{libexec}}/DesktopPet.app"],
              writable_paths: ["{{libexec}}/DesktopPet.app"]
          run "/usr/bin/codesign", args: ["--verify", "--deep", "--strict", "{{libexec}}/DesktopPet.app"]
          # No install request, locator/profile registration, adoption, or daemon action.
          # Migration's engine alone switches ownership after source quiescence.
          run "/usr/bin/ruby",
              args: ["--disable=rubyopt", "{{libexec}}/derive-rig-native.rb", "{{libexec}}/DesktopPet.app",
                     "#{sidecar_sha}", "#{core_revision}"],
              env: { "RUBYLIB" => "", "RUBYOPT" => "" },
              writable_paths: ["{{libexec}}"]
        end

        def caveats
          <<~EOS
            Start manually with: desktop-pet start
            Inspect the selected profile with: desktop-pet profile show --json
            This installs only the app and CLI under Homebrew's prefix, not /Applications.
            Installation does not start/stop the app or register/adopt a profile or locator.
            Herdr integration is optional: herdr plugin install hanbong5938/herdr-desktop-pet
            Tern integration is optional: tern plugin install https://github.com/hanbong5938/tern-desktop-pet
            Plugins do not install a patched host or start the native app by installation alone.
            The app is ad-hoc signed, not Developer ID signed or notarized.
            Existing Herdr desktop-pet installations require the explicit bridge migration;
            installing this formula alone does not retire them or migrate their profiles.
          EOS
        end

        test do
          # Pack validation initializes AppKit, which aborts in brew test's session.
          assert_equal "desktop-pet \#{version}", shell_output("\#{bin}/desktop-pet --version").strip
          system "/usr/bin/codesign", "--verify", "--deep", "--strict", libexec/"DesktopPet.app"
        end
      end
    RUBY
    text.sub("    __EMBEDDED_HELPER__\n", embedded)
  end

  def self.run(output)
    check(output.start_with?("/") && File.expand_path(output) == output && output != "/", "output must be a new canonical absolute directory")
    check(!File.exist?(output) && !File.symlink?(output), "output already exists; no candidate or tap is overwritten")
    parent = File.dirname(output)
    check(File.directory?(parent) && File.realpath(parent) == parent, "output parent must already be a physical directory")
    stage = Dir.mktmpdir(".desktop-pet-homebrew-materialize-", parent)
    begin
      evidence = File.join(stage, "release-evidence")
      Dir.mkdir(evidence, 0700)
      release = fetch_json("#{API}/releases/tags/v0.4.0", File.join(evidence, "release.json"))
      assets = asset_map(release)
      digests = {}
      ASSETS.each do |name|
        path = File.join(evidence, name)
        fetch(assets[name]["browser_download_url"], path, assets[name]["size"])
        check(File.size(path) == assets[name]["size"], "download size disagrees with observed API asset: #{name}")
        digests[name] = Digest::SHA256.file(path).hexdigest
        advertised = assets[name]["digest"]
        check(advertised.nil? || advertised == "sha256:#{digests[name]}", "GitHub asset digest mismatch: #{name}")
      end
      sums = checksums(File.join(evidence, "SHA256SUMS"), [ARCHIVE, SIDECAR, INSTALLER])
      sums.each { |name, digest| check(digests[name] == digest, "release SHA256SUMS mismatch: #{name}") }
      [ARCHIVE, INSTALLER].each do |name|
        check(checksums(File.join(evidence, "#{name}.sha256"), [name])[name] == digests[name], "individual checksum mismatch: #{name}")
      end
      core_revision = revision(evidence)
      record = JSON.parse(File.binread(File.join(evidence, SIDECAR)))
      hashes = DesktopPetDeploymentRecord.validate(record, core_revision)
      validate_archive(File.join(evidence, ARCHIVE), hashes)
      # Detect asset replacement while observing; never select a mutable branch/local digest.
      final = fetch_json("#{API}/releases/tags/v0.4.0", File.join(evidence, "release-final.json"))
      check(final["id"] == release["id"] && asset_map(final) == assets, "published release asset identity changed during materialization")
      Dir.mkdir(File.join(stage, "Formula"), 0700)
      File.write(File.join(stage, "Formula", "desktop-pet.rb"), formula(digests[ARCHIVE], digests[SIDECAR], core_revision))
      manifest = {"version" => 1, "repository" => REPOSITORY, "tag" => "v0.4.0", "release_id" => release["id"],
        "core_revision" => core_revision, "protocol" => 4, "observation_mode" => 2, "integration_api" => 1,
        "release_record_sha256" => Digest::SHA256.file(File.join(evidence, "release.json")).hexdigest,
        "assets" => ASSETS.map { |name| {"name" => name, "id" => assets[name]["id"], "size" => assets[name]["size"],
          "url" => assets[name]["browser_download_url"], "sha256" => digests[name]} }}
      File.write(File.join(stage, "core-release.json"), JSON.pretty_generate(manifest) + "\n")
      check(!File.exist?(output) && !File.symlink?(output), "output appeared during materialization")
      File.rename(stage, output)
      stage = nil
      puts "Materialized #{output}/Formula/desktop-pet.rb from public release #{release['id']} (#{core_revision}). No tap or installation was modified."
    ensure
      FileUtils.remove_entry_secure(stage) if stage && File.exist?(stage)
    end
  end
end

if $PROGRAM_NAME == __FILE__
  abort "Usage: /usr/bin/ruby migration/integration-cutover/homebrew/materialize-release.rb NEW_ABSOLUTE_OUTPUT_DIRECTORY" unless ARGV.length == 1
  File.umask(0077)
  begin
    DesktopPetHomebrewMaterializer.run(ARGV.fetch(0))
  rescue StandardError => error
    abort "Homebrew formula materialization refused: #{error.message}"
  end
end
