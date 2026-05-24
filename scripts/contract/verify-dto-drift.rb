#!/usr/bin/env ruby
# frozen_string_literal: true

require "json"
require "pathname"

ROOT = Pathname.new(__dir__).join("..", "..").expand_path

def read(path)
  ROOT.join(path).read
end

def fail_contract(message)
  warn("Contract drift: #{message}")
  exit(1)
end

def assert_file_contains(path, needle)
  text = read(path)
  return if text.include?(needle)

  fail_contract("expected #{path} to contain #{needle.inspect}")
end

def assert_dart_models_contains(needle)
  paths = [
    "lib/flutter/vesper_player_platform_interface/lib/src/models.dart",
    *Dir.glob(ROOT.join("lib/flutter/vesper_player_platform_interface/lib/src/models/*.dart"))
        .map { |path| path.delete_prefix("#{ROOT}/") }
  ]
  found = paths.any? { |path| read(path).include?(needle) }
  return if found

  fail_contract("expected Flutter platform interface models to contain #{needle.inspect}")
end

def assert_tree_contains(path, needle)
  root = ROOT.join(path)
  found = Dir.glob(root.join("**", "*")).any? do |candidate|
    File.file?(candidate) && File.read(candidate).include?(needle)
  end
  return if found

  fail_contract("expected #{path} tree to contain #{needle.inspect}")
end

def assert_json_keys(path, keys)
  parsed = JSON.parse(read(path))
  keys.each do |key|
    fail_contract("expected #{path} JSON key #{key.inspect}") unless parsed.key?(key)
  end
end

def camel_to_pascal(value)
  value.split("_").map { |part| part[0].upcase + part[1..] }.join
end

def camel_to_rust_variant(value)
  value.gsub(/([a-z\d])([A-Z])/, "\\1_\\2")
       .split("_")
       .map { |part| part[0].upcase + part[1..] }
       .join
end

def kotlin_variant(value)
  overrides = {
    "mpegTs" => "MpegTs",
    "airPlay" => "AirPlay",
    "dlna" => "Dlna",
    "hls" => "Hls"
  }
  overrides.fetch(value) { camel_to_pascal(value) }
end

def swift_case(value)
  ".#{value}"
end

def check_wire_values(label, values, expectations)
  values.each do |value|
    expectations.each do |path, needle_builder|
      needle = needle_builder.call(value)
      if path == "flutter_models"
        assert_dart_models_contains(needle)
      elsif path.end_with?("/")
        assert_tree_contains(path.delete_suffix("/"), needle)
      else
        assert_file_contains(path, needle)
      end
    end
  end
  puts "checked #{label}: #{values.join(', ')}"
end

player_error = JSON.parse(read("fixtures/contracts/player_error.json"))
assert_json_keys(
  "fixtures/contracts/player_error.json",
  %w[message code category retriable details]
)

check_wire_values(
  "player error code/category",
  [player_error.fetch("code"), player_error.fetch("category")],
  {
    "flutter_models" => ->(v) { v },
    "lib/android/vesper-player-kit/src/main/java/io/github/ikaros/vesper/player/android/VesperPlayerError.kt" => ->(v) { "\"#{v}\"" },
    "lib/ios/VesperPlayerKit/Sources/VesperPlayerKit/PlayerBridge.swift" => ->(v) { "case #{v}" },
    "crates/model/player-model/src/error.rs" => ->(v) { camel_to_rust_variant(v) },
    "crates/ffi/player-ffi/src/c_api/" => ->(v) { camel_to_rust_variant(v) },
    "crates/ffi/player-ffi-ios/src/" => ->(v) { camel_to_rust_variant(v) }
  }
)

plugin_diagnostics = JSON.parse(read("fixtures/contracts/plugin_diagnostics.json"))
plugin_diagnostics.each do |record|
  %w[path pluginName pluginKind status participation message capability].each do |key|
    fail_contract("expected plugin diagnostic key #{key.inspect}") unless record.key?(key)
  end
end

plugin_values = plugin_diagnostics.flat_map do |record|
  [
    record.fetch("status"),
    record.fetch("participation"),
    record.fetch("capability").fetch("kind")
  ]
end.uniq

check_wire_values(
  "plugin diagnostics",
  plugin_values,
  {
    "fixtures/contracts/plugin_diagnostics.json" => ->(v) { "\"#{v}\"" },
    "flutter_models" => ->(v) { v },
    "crates/core/player-runtime/src/lib.rs" => ->(v) { camel_to_rust_variant(v) },
    "crates/ffi/player-ffi/src/c_api/" => ->(v) { camel_to_rust_variant(v) }
  }
)

download_snapshot = JSON.parse(read("fixtures/contracts/download_task_snapshot.json"))
%w[taskId assetId source profile state progress assetIndex error].each do |key|
  fail_contract("expected download snapshot key #{key.inspect}") unless download_snapshot.key?(key)
end

download_values = [
  download_snapshot.fetch("source").fetch("contentFormat"),
  download_snapshot.fetch("state"),
  download_snapshot.fetch("assetIndex").fetch("contentFormat"),
  download_snapshot.fetch("assetIndex").fetch("streams").first.fetch("kind")
].compact.uniq

check_wire_values(
  "download snapshot",
  download_values,
  {
    "lib/flutter/vesper_player_platform_interface/lib/src/download_models.dart" => ->(v) { v },
    "lib/android/vesper-player-kit/src/main/java/io/github/ikaros/vesper/player/android/" => ->(v) { kotlin_variant(v) },
    "lib/ios/VesperPlayerKit/Sources/VesperPlayerKit/" => ->(v) { swift_case(v) },
    "crates/core/player-download/src/download/types.rs" => ->(v) { camel_to_rust_variant(v) },
    "crates/ffi/player-ffi-ios/src/" => ->(v) { camel_to_rust_variant(v) }
  }
)

download_output_values = [download_snapshot.fetch("profile").fetch("targetOutputFormat")].compact
check_wire_values(
  "download output format",
  download_output_values,
  {
    "lib/flutter/vesper_player_platform_interface/lib/src/download_models.dart" => ->(v) { v },
    "lib/android/vesper-player-kit/src/main/java/io/github/ikaros/vesper/player/android/" => ->(v) { kotlin_variant(v) },
    "lib/ios/VesperPlayerKit/Sources/VesperPlayerKit/" => ->(v) { swift_case(v) },
    "crates/plugin/player-plugin/src/processor.rs" => ->(v) { camel_to_rust_variant(v) },
    "crates/ffi/player-ffi-ios/src/" => ->(v) { camel_to_rust_variant(v) }
  }
)

system_playback = JSON.parse(read("fixtures/contracts/system_playback_configuration.json"))
%w[enabled backgroundMode showSystemControls showSeekActions metadata controls].each do |key|
  fail_contract("expected system playback key #{key.inspect}") unless system_playback.key?(key)
end
system_values = [
  system_playback.fetch("backgroundMode"),
  *system_playback.fetch("controls").fetch("compactButtons").map { |button| button.fetch("kind") }
].uniq

check_wire_values(
  "system playback",
  system_values,
  {
    "flutter_models" => ->(v) { v },
    "lib/android/vesper-player-kit/src/main/java/io/github/ikaros/vesper/player/android/PlayerBridge.kt" => ->(v) { kotlin_variant(v) },
    "lib/ios/VesperPlayerKit/Sources/VesperPlayerKit/PlayerBridge.swift" => ->(v) { swift_case(v) }
  }
)

external_values = %w[cast dlna auto always never hls routeConnected routeDisconnected loaded playing paused stopped suspended error discoveryDiagnostic]
check_wire_values(
  "external playback",
  external_values,
  {
    "flutter_models" => ->(v) { v },
    "lib/android/vesper-player-kit-external-playback/src/main/java/io/github/ikaros/vesper/player/android/external/VesperExternalPlaybackModels.kt" => ->(v) { kotlin_variant(v) },
    "lib/flutter/vesper_player_external_playback/android/src/main/kotlin/io/github/ikaros/vesper/player/flutter/externalplayback/VesperPlayerExternalPlaybackPlugin.kt" => ->(v) { v }
  }
)

check_wire_values(
  "external fallback default",
  %w[mpegTs],
  {
    "flutter_models" => ->(v) { v },
    "lib/android/vesper-player-kit-external-playback/src/main/java/io/github/ikaros/vesper/player/android/external/VesperExternalPlaybackModels.kt" => ->(v) { kotlin_variant(v) }
  }
)

check_wire_values(
  "external playback result status",
  %w[success unavailable unsupported failed],
  {
    "flutter_models" => ->(v) { v },
    "lib/flutter/vesper_player_external_playback/android/src/main/kotlin/io/github/ikaros/vesper/player/flutter/externalplayback/VesperPlayerExternalPlaybackPlugin.kt" => ->(v) { v },
    "lib/android/vesper-player-kit-external-playback/src/main/java/io/github/ikaros/vesper/player/android/external/VesperExternalPlaybackModels.kt" => ->(v) { kotlin_variant(v) }
  }
)

puts "DTO contract drift check passed."
